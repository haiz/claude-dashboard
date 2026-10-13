import Foundation

/// One account as auto switch sees it, built by the view model from `AccountUsageState`.
struct AutoSwitchAccount: Equatable {
    let id: UUID
    /// Email when known, else the account name. Shown in the footer.
    let displayName: String
    let usage: UsageData?
    let availability: SwitchAvailability?
    /// `Account.status == .active`.
    let isUsable: Bool
    /// Claude Code uses this account now.
    let isActive: Bool
    /// The last fetch succeeded (`AccountUsageState.error == nil`). A failed fetch keeps the
    /// previous usage on the card; deciding on those numbers would act on stale data.
    var isFresh = true

    var fiveHour: Double { usage?.fiveHour.utilization ?? 100 }
    var sevenDay: Double { usage?.sevenDay.utilization ?? 100 }
}

struct AutoSwitchThresholds: Equatable {
    let fiveHour: Double
    let sevenDay: Double
}

/// The account to switch to as soon as its 5h window resets. `baseline` is its 5h
/// `resetsAt` when ticked; nil means the window had not started.
struct AutoSwitchPriority: Equatable {
    let accountId: UUID
    let baseline: Date?
}

/// After an auto switch lands on an account already at its threshold in a window, that
/// window's trigger is raised to 100 until it is used up or resets. Each window keeps
/// its `resetsAt` from when it was locked.
struct AutoSwitchLock: Equatable {
    struct Window: Equatable { let resetsAt: Date? }
    let accountId: UUID
    var fiveHour: Window?
    var sevenDay: Window?
}

struct AutoSwitchSnapshot {
    var accounts: [AutoSwitchAccount]
    var thresholds: AutoSwitchThresholds
    var lock: AutoSwitchLock?
    var priority: AutoSwitchPriority?
    /// Accounts in the failure cooldown.
    var excluded: Set<UUID>
    var now: Date
}

enum AutoSwitchReason: Equatable {
    case priorityReset
    case threshold(tier: Int)
}

enum AutoSwitchStayReason: Equatable {
    /// Claude Code's account is not on the dashboard, or its usage never loaded or is stale.
    case noActiveAccount
    case belowThreshold
    case noCandidate
}

enum AutoSwitchDecision: Equatable {
    case stay(AutoSwitchStayReason)
    case switchTo(UUID, AutoSwitchReason)
}

/// Which account Claude Code should move to, if any. Pure: the spec's rules
/// (`docs/superpowers/specs/2026-10-13-auto-switch-design.md`) and nothing else.
///
///     let d = AutoSwitchPlanner.decide(snapshot, pickIndex: { Int.random(in: 0..<$0) })
///     if case .switchTo(let id, _) = d { ... }
enum AutoSwitchPlanner {
    static let tierFiveHourLimit = 91.0
    static let tierSevenDayLimit = 99.0
    static let scoreSevenDayCap = 98.0
    static let groupRatio = 0.85
    /// `resetsAt` drifts by sub-seconds between fetches; a real reset moves it by hours.
    static let resetTolerance: TimeInterval = 60

    static func isTriggered(_ usage: UsageData, thresholds: AutoSwitchThresholds) -> Bool {
        usage.fiveHour.utilization >= thresholds.fiveHour
            || usage.sevenDay.utilization >= thresholds.sevenDay
    }

    static func isInWarningZone(_ usage: UsageData, thresholds: AutoSwitchThresholds) -> Bool {
        usage.fiveHour.utilization >= thresholds.fiveHour - 3
            || usage.sevenDay.utilization >= thresholds.sevenDay - 1
    }

    /// Days elapsed in the 7d window times the 7d percent left below 98. A week that has
    /// not started (`resetsAt == nil`) has 0 days elapsed.
    static func score(_ usage: UsageData, now: Date) -> Double {
        let elapsed = usage.sevenDay.resetsAt
            .map { min(max(7 - $0.timeIntervalSince(now) / 86_400, 0), 7) } ?? 0
        return elapsed * max(0, scoreSevenDayCap - usage.sevenDay.utilization)
    }

    /// True when a window that had `baseline` as its `resetsAt` has since reset: it now
    /// reports nil (not started) or a time more than `resetTolerance` later. A nil
    /// baseline means it had not started, so there is nothing to wait for.
    static func hasReset(current: Date?, baseline: Date?) -> Bool {
        guard let baseline, let current else { return true }
        return current.timeIntervalSince(baseline) > resetTolerance
    }

    static func effectiveThresholds(for id: UUID, base: AutoSwitchThresholds,
                                    lock: AutoSwitchLock?) -> AutoSwitchThresholds {
        guard let lock, lock.accountId == id else { return base }
        return AutoSwitchThresholds(fiveHour: lock.fiveHour == nil ? base.fiveHour : 100,
                                    sevenDay: lock.sevenDay == nil ? base.sevenDay : 100)
    }

    /// The lock for an account just switched to: the windows already at their threshold.
    static func makeLock(for id: UUID, usage: UsageData,
                         thresholds: AutoSwitchThresholds) -> AutoSwitchLock? {
        let five = usage.fiveHour.utilization >= thresholds.fiveHour
            ? AutoSwitchLock.Window(resetsAt: usage.fiveHour.resetsAt) : nil
        let seven = usage.sevenDay.utilization >= thresholds.sevenDay
            ? AutoSwitchLock.Window(resetsAt: usage.sevenDay.resetsAt) : nil
        guard five != nil || seven != nil else { return nil }
        return AutoSwitchLock(accountId: id, fiveHour: five, sevenDay: seven)
    }

    /// Drops windows that are used up (100) or have reset, and the whole lock once
    /// Claude Code uses another account.
    static func updatedLock(_ lock: AutoSwitchLock?, active: AutoSwitchAccount?) -> AutoSwitchLock? {
        guard var lock, let active, active.id == lock.accountId else { return nil }
        guard let usage = active.usage else { return lock }
        if let w = lock.fiveHour,
           usage.fiveHour.utilization >= 100 || hasReset(current: usage.fiveHour.resetsAt, baseline: w.resetsAt) {
            lock.fiveHour = nil
        }
        if let w = lock.sevenDay,
           usage.sevenDay.utilization >= 100 || hasReset(current: usage.sevenDay.resetsAt, baseline: w.resetsAt) {
            lock.sevenDay = nil
        }
        return lock.fiveHour == nil && lock.sevenDay == nil ? nil : lock
    }

    static func isCandidate(_ a: AutoSwitchAccount, excluded: Set<UUID>) -> Bool {
        !a.isActive && a.availability == .ready && a.isUsable && a.isFresh && a.usage != nil
            && !excluded.contains(a.id) && a.fiveHour < 100 && a.sevenDay < 100
    }

    /// `pickIndex(n)` returns an index in `0..<n`; production passes `Int.random(in:)`.
    static func decide(_ s: AutoSwitchSnapshot, pickIndex: (Int) -> Int) -> AutoSwitchDecision {
        guard let active = s.accounts.first(where: \.isActive), active.isFresh,
              let activeUsage = active.usage else {
            return .stay(.noActiveAccount)
        }
        let candidates = s.accounts.filter { isCandidate($0, excluded: s.excluded) }

        if let priority = s.priority,
           let target = candidates.first(where: { $0.id == priority.accountId }),
           target.sevenDay < tierSevenDayLimit,
           hasReset(current: target.usage?.fiveHour.resetsAt, baseline: priority.baseline) {
            return .switchTo(target.id, .priorityReset)
        }

        let limits = effectiveThresholds(for: active.id, base: s.thresholds, lock: s.lock)
        guard isTriggered(activeUsage, thresholds: limits) else { return .stay(.belowThreshold) }

        let tier1 = candidates.filter { $0.fiveHour < tierFiveHourLimit && $0.sevenDay < tierSevenDayLimit }
        if !tier1.isEmpty {
            let scored = tier1.map { ($0, $0.usage.map { score($0, now: s.now) } ?? 0) }
            let top = scored.map(\.1).max() ?? 0
            let group = scored.filter { $0.1 >= groupRatio * top }.map(\.0)
            let index = min(max(pickIndex(group.count), 0), group.count - 1)
            return .switchTo(group[index].id, .threshold(tier: 1))
        }
        // Tier 1 is empty, so every candidate under 99 on 7d has 5h >= 91.
        if let pick = candidates.filter({ $0.sevenDay < tierSevenDayLimit }).min(by: { $0.fiveHour < $1.fiveHour }) {
            return .switchTo(pick.id, .threshold(tier: 2))
        }
        if let pick = candidates.min(by: { ($0.sevenDay, $0.fiveHour) < ($1.sevenDay, $1.fiveHour) }) {
            return .switchTo(pick.id, .threshold(tier: 3))
        }
        return .stay(.noCandidate)
    }
}
