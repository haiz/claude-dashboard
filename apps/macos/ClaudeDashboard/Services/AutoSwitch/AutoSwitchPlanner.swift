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
    /// From `CriticalPlanner.assess`; `.none` when not critical or unknown.
    var critical: CriticalAssessment = .none
    /// The newest 5h `usage_logs` rows, oldest first. Filled for Claude Code's account only:
    /// the near-reset rule reads its recent pace.
    var fiveHourLog: [UsageLogEntry] = []

    var fiveHour: Double { usage?.fiveHour.utilization ?? 100 }
    var sevenDay: Double { usage?.sevenDay.utilization ?? 100 }
}

struct AutoSwitchThresholds: Equatable {
    let fiveHour: Double
    let sevenDay: Double
}

/// The home account: Claude Code moves back to it each time its 5h window resets.
/// `baseline` is its 5h `resetsAt` when ticked, or when Claude Code last used it; nil
/// means it was ticked before its window started.
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

/// After a critical switch or a manual Switch, Claude Code stays on that account until
/// `until` (its 5h reset): rules 1 and 3 do not move it while the hold lasts. Using the
/// account lowers its critical level, which would otherwise pull Claude Code to the next
/// critical account mid window. The home reset and the thresholds still apply.
struct AutoSwitchHold: Equatable {
    let accountId: UUID
    var until: Date
}

struct AutoSwitchSnapshot {
    var accounts: [AutoSwitchAccount]
    var thresholds: AutoSwitchThresholds
    var lock: AutoSwitchLock?
    var hold: AutoSwitchHold? = nil
    var priority: AutoSwitchPriority?
    /// Accounts in the failure cooldown.
    var excluded: Set<UUID>
    var now: Date
}

enum AutoSwitchReason: Equatable {
    case priorityReset
    case critical(CriticalLevel)
    case threshold(tier: Int)
}

enum AutoSwitchStayReason: Equatable {
    /// Claude Code's account is not on the dashboard, or its usage never loaded or is stale.
    case noActiveAccount
    case belowThreshold
    case noCandidate
    /// Over the 5h threshold, but the 5h window resets within minutes at a pace that fits.
    case nearReset
}

enum AutoSwitchDecision: Equatable {
    case stay(AutoSwitchStayReason)
    case switchTo(UUID, AutoSwitchReason)
}

/// Which account Claude Code should move to, if any. Pure: the spec's rules
/// (`docs/superpowers/specs/2026-10-13-auto-switch-design.md`) and nothing else.
/// Priority order: high critical, home, critical, then the threshold tiers. A hold on the
/// active account turns off the two critical rules until it ends. The thresholds wait out
/// the last minutes of the home or most critical account's 5h window (`waitsForReset`).
///
///     let d = AutoSwitchPlanner.decide(snapshot, pickIndex: { Int.random(in: 0..<$0) })
///     if case .switchTo(let id, _) = d { ... }
enum AutoSwitchPlanner {
    static let tierFiveHourLimit = 91.0
    static let tierSevenDayLimit = 99.0
    static let scoreSevenDayCap = 98.0
    static let scoreMinDaysLeft = 0.25
    static let groupRatio = 0.85
    /// `resetsAt` drifts by sub-seconds between fetches; a real reset moves it by hours.
    static let resetTolerance: TimeInterval = 60
    static let fiveHourWindow: TimeInterval = 5 * 3600
    /// A critical target's 5h counts as free at or under this (an Auto Ping leaves about 1%).
    static let freeFiveHourLimit = 2.0
    /// The near-reset rule looks at the last minutes of a 5h window, and keeps this much
    /// of the 5h quota unused as a margin.
    static let nearResetWindow: TimeInterval = 5 * 60
    static let nearResetMargin = 1.0

    static func isTriggered(_ usage: UsageData, thresholds: AutoSwitchThresholds) -> Bool {
        usage.fiveHour.utilization >= thresholds.fiveHour
            || usage.sevenDay.utilization >= thresholds.sevenDay
    }

    static func isInWarningZone(_ usage: UsageData, thresholds: AutoSwitchThresholds) -> Bool {
        usage.fiveHour.utilization >= thresholds.fiveHour - 3
            || usage.sevenDay.utilization >= thresholds.sevenDay - 1
    }

    /// Wider than the warning zone: the footer toggle appears early enough to turn auto
    /// switch on before the fast poll would start.
    static func isInToggleZone(_ usage: UsageData, thresholds: AutoSwitchThresholds) -> Bool {
        usage.fiveHour.utilization >= thresholds.fiveHour - 15
            || usage.sevenDay.utilization >= thresholds.sevenDay - 3
    }

    /// The 7d percent left below 98 per day left in the week: what the account must burn
    /// to use its quota before the reset. Days left are floored at `scoreMinDaysLeft`. A week
    /// that has not started (`resetsAt == nil`) scores 0, keeping the account in reserve.
    static func score(_ usage: UsageData, now: Date) -> Double {
        guard let resetsAt = usage.sevenDay.resetsAt else { return 0 }
        let daysLeft = min(max(resetsAt.timeIntervalSince(now) / 86_400, 0), 7)
        return max(0, scoreSevenDayCap - usage.sevenDay.utilization) / max(daysLeft, scoreMinDaysLeft)
    }

    /// True when a window that had `baseline` as its `resetsAt` has since reset: it now
    /// reports nil (not started) or a time more than `resetTolerance` later. A nil
    /// baseline means it had not started, so there is nothing to wait for.
    static func hasReset(current: Date?, baseline: Date?) -> Bool {
        guard let baseline, let current else { return true }
        return current.timeIntervalSince(baseline) > resetTolerance
    }

    /// The baseline of the ticked account while Claude Code uses it: its 5h `resetsAt`, or,
    /// before its window starts, the earliest time a window started from now could reset.
    static func priorityBaseline(_ usage: UsageData, now: Date) -> Date {
        usage.fiveHour.resetsAt ?? now + fiveHourWindow
    }

    /// True when the ticked account's 5h window has reset since `baseline`. A nil `current`
    /// counts only once the baseline has passed: a baseline still ahead was set when Claude
    /// Code left it before its window started.
    static func isPriorityDue(current: Date?, baseline: Date?, now: Date) -> Bool {
        if current == nil, let baseline, baseline > now { return false }
        return hasReset(current: current, baseline: baseline)
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

    /// A critical candidate whose 5h window is free and whose week is not used up.
    static func isCriticalTarget(_ a: AutoSwitchAccount, excluded: Set<UUID>) -> Bool {
        guard a.critical.level != .none, isCandidate(a, excluded: excluded),
              a.sevenDay < tierSevenDayLimit, let usage = a.usage else { return false }
        return usage.fiveHour.resetsAt == nil || usage.fiveHour.utilization <= freeFiveHourLimit
    }

    /// True when Claude Code should sit out the last minutes of its 5h window instead of a
    /// threshold switch it would undo at the reset. Only the home account (no high critical
    /// target waiting) and the most critical account qualify, only for the 5h threshold, and
    /// only when the 5h used in the last N+1 minutes (N = minutes left) fits in what is left,
    /// less `nearResetMargin`. No log row at or before that point means no wait.
    ///
    ///     // 5 minutes left at 95%, 91% six minutes ago: 4 <= 100 - 95 - 1, so wait.
    static func waitsForReset(_ active: AutoSwitchAccount, limits: AutoSwitchThresholds,
                              targets: [AutoSwitchAccount], _ s: AutoSwitchSnapshot) -> Bool {
        let others = targets.filter { $0.id != active.id }
        let qualifies = active.id == s.priority?.accountId
            ? !others.contains { $0.critical.level == .high }
            : active.critical.level != .none && !others.contains { $0.critical.waste > active.critical.waste }
        guard qualifies, let usage = active.usage, usage.sevenDay.utilization < limits.sevenDay,
              let resetsAt = usage.fiveHour.resetsAt else { return false }
        let left = resetsAt.timeIntervalSince(s.now)
        guard left > 0, left <= nearResetWindow else { return false }
        let cutoff = s.now - left - 60
        guard let earlier = active.fiveHourLog
            .filter({ $0.recordedAt <= cutoff && abs($0.resetsAt.timeIntervalSince(resetsAt)) <= resetTolerance })
            .max(by: { $0.recordedAt < $1.recordedAt }) else { return false }
        let used = usage.fiveHour.utilization - earlier.utilization
        return used <= 100 - usage.fiveHour.utilization - nearResetMargin
    }

    /// `pickIndex(n)` returns an index in `0..<n`; production passes `Int.random(in:)`.
    static func decide(_ s: AutoSwitchSnapshot, pickIndex: (Int) -> Int) -> AutoSwitchDecision {
        guard let active = s.accounts.first(where: \.isActive), active.isFresh,
              let activeUsage = active.usage else {
            return .stay(.noActiveAccount)
        }
        let candidates = s.accounts.filter { isCandidate($0, excluded: s.excluded) }
        let targets = s.accounts.filter { isCriticalTarget($0, excluded: s.excluded) }
        let activeIsHigh = active.critical.level == .high
        let isHeld = s.hold.map { $0.accountId == active.id && s.now < $0.until } ?? false

        // 1. High critical beats everything, home included, unless Claude Code is held.
        if !activeIsHigh, !isHeld,
           let target = targets.filter({ $0.critical.level == .high }).max(by: { $0.critical.waste < $1.critical.waste }) {
            return .switchTo(target.id, .critical(.high))
        }

        // 2. Home, unless Claude Code is on a high critical account.
        if !activeIsHigh, let priority = s.priority,
           let target = candidates.first(where: { $0.id == priority.accountId }),
           target.sevenDay < tierSevenDayLimit,
           isPriorityDue(current: target.usage?.fiveHour.resetsAt, baseline: priority.baseline, now: s.now) {
            return .switchTo(target.id, .priorityReset)
        }

        // 3. Critical beats an ordinary account that is not home, unless Claude Code is held.
        if !isHeld, active.critical.level == .none, active.id != s.priority?.accountId,
           let target = targets.max(by: { $0.critical.waste < $1.critical.waste }) {
            return .switchTo(target.id, .critical(target.critical.level))
        }

        let limits = effectiveThresholds(for: active.id, base: s.thresholds, lock: s.lock)
        guard isTriggered(activeUsage, thresholds: limits) else { return .stay(.belowThreshold) }
        if waitsForReset(active, limits: limits, targets: targets, s) { return .stay(.nearReset) }

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
