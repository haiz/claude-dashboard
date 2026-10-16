import Foundation

/// The footer's report of the last evaluation that did something.
struct AutoSwitchEvent: Equatable {
    enum Kind: Equatable {
        case switched(AutoSwitchReason)
        case failed(String)
        case noCandidate
    }
    let kind: Kind
    let accountName: String?
    let date: Date

    var text: String {
        let name = accountName ?? "account"
        switch kind {
        case .switched(.priorityReset): return "Switched to \(name) after its 5h reset"
        case .switched(.critical(let level)): return "Switched to \(name): \(level.label) account"
        case .switched: return "Auto-switched to \(name)"
        case .failed(let reason): return "Could not switch to \(name): \(reason)"
        case .noCandidate: return "No account to switch to"
        }
    }

    var isFailure: Bool {
        if case .failed = kind { return true }
        return false
    }
}

/// Runs `AutoSwitchPlanner` against each refresh and performs its decision. Holds what the
/// planner must not: the lock (in memory only), the 10-minute failure cooldown, the
/// footer event and the Switch Log rows. The switch itself is the caller's closure, so
/// this never touches the Keychain and never opens a browser.
///
///     let controller = AutoSwitchController(settings: settings, logStore: AutoSwitchLogStore())
///     await controller.evaluate(accounts) { id in try await vm.performAutoSwitch(to: id) }
@MainActor
final class AutoSwitchController: ObservableObject {
    static let failureCooldown: TimeInterval = 600

    let settings: AutoSwitchSettings
    let logStore: AutoSwitchLogStore
    @Published private(set) var lastEvent: AutoSwitchEvent?
    private(set) var lock: AutoSwitchLock?
    private var cooldownUntil: [UUID: Date] = [:]
    /// Set when a switch fails because of Claude Code's current login, not the target:
    /// every target would fail the same way, so auto switch pauses as a whole.
    private var pausedUntil: Date?
    /// "No candidate" repeats on every refresh until something changes; it is logged once.
    private var loggedNoCandidate = false
    private let now: () -> Date
    private let pickIndex: (Int) -> Int

    init(settings: AutoSwitchSettings,
         logStore: AutoSwitchLogStore,
         now: @escaping () -> Date = Date.init,
         pickIndex: @escaping (Int) -> Int = { Int.random(in: 0..<$0) }) {
        self.settings = settings
        self.logStore = logStore
        self.now = now
        self.pickIndex = pickIndex
    }

    /// A manual switch ends the lock at once.
    func clearLock() { lock = nil }

    /// True while auto switch is on and the active account is in the warning zone.
    func shouldFastPoll(_ accounts: [AutoSwitchAccount]) -> Bool {
        guard settings.isEnabled, let active = accounts.first(where: \.isActive), active.isFresh,
              let usage = active.usage else { return false }
        return AutoSwitchPlanner.isInWarningZone(usage, thresholds: settings.thresholds)
    }

    /// Returns the account switched to, or nil.
    @discardableResult
    func evaluate(_ accounts: [AutoSwitchAccount],
                  perform: (UUID) async throws -> Void) async -> UUID? {
        updatePriority(accounts)
        guard settings.isEnabled else { return nil }

        let t = now()
        if let pausedUntil, pausedUntil > t { return nil }
        pausedUntil = nil
        cooldownUntil = cooldownUntil.filter { $0.value > t }
        lock = AutoSwitchPlanner.updatedLock(lock, active: accounts.first(where: \.isActive))
        let snapshot = AutoSwitchSnapshot(accounts: accounts, thresholds: settings.thresholds, lock: lock,
                                          priority: settings.priority, excluded: Set(cooldownUntil.keys), now: t)

        let decision = AutoSwitchPlanner.decide(snapshot, pickIndex: pickIndex)
        let active = accounts.first(where: \.isActive)
        if decision != .stay(.noCandidate) { loggedNoCandidate = false }
        switch decision {
        case .stay(.noCandidate):
            lastEvent = AutoSwitchEvent(kind: .noCandidate, accountName: nil, date: t)
            if !loggedNoCandidate {
                loggedNoCandidate = true
                await logStore.insert(date: t, outcome: .noCandidate, fromName: active?.displayName,
                                      toName: nil, reason: triggerText(active), detail: nil)
            }
            return nil
        case .stay:
            return nil
        case .switchTo(let id, let reason):
            let target = accounts.first { $0.id == id }
            let reasonText: String
            switch reason {
            case .priorityReset: reasonText = "Ticked account's 5h window reset"
            case .critical(let level):
                let kind = level == .high ? "High critical" : "Critical"
                let waste = Self.percent(target?.critical.waste ?? 0)
                reasonText = "\(kind) account's 5h window is free, \(waste) of 7d would go unused"
            case .threshold(let tier): reasonText = "\(triggerText(active)), tier \(tier)"
            }
            do {
                try await perform(id)
            } catch {
                if Self.isActiveLoginError(error) {
                    pausedUntil = t.addingTimeInterval(Self.failureCooldown)
                } else {
                    cooldownUntil[id] = t.addingTimeInterval(Self.failureCooldown)
                }
                lastEvent = AutoSwitchEvent(kind: .failed(Self.describe(error)),
                                            accountName: target?.displayName, date: t)
                await logStore.insert(date: t, outcome: .failed, fromName: active?.displayName,
                                      toName: target?.displayName, reason: reasonText,
                                      detail: Self.describe(error))
                return nil
            }
            lock = target?.usage.flatMap {
                AutoSwitchPlanner.makeLock(for: id, usage: $0, thresholds: settings.thresholds)
            }
            lastEvent = AutoSwitchEvent(kind: .switched(reason), accountName: target?.displayName, date: t)
            await logStore.insert(date: t, outcome: .switched, fromName: active?.displayName,
                                  toName: target?.displayName, reason: reasonText,
                                  detail: target.map { "\($0.displayName): \(Self.usageText($0))" })
            return id
        }
    }

    /// Drops a tick on a removed account. While the ticked account is active, moves its
    /// baseline to the window Claude Code uses, so leaving it waits for that window's reset.
    private func updatePriority(_ accounts: [AutoSwitchAccount]) {
        guard let priority = settings.priority else { return }
        guard let home = accounts.first(where: { $0.id == priority.accountId }) else {
            settings.setPriority(nil)
            return
        }
        guard home.isActive, home.isFresh, let usage = home.usage else { return }
        let baseline = AutoSwitchPlanner.priorityBaseline(usage, now: now())
        if baseline != priority.baseline {
            settings.setPriority(AutoSwitchPriority(accountId: home.id, baseline: baseline))
        }
    }

    /// The active account's windows at or over their limit (the lock's 100 included), e.g.
    /// "5h 96% >= 95%". Falls back to both windows' usage when none is over.
    private func triggerText(_ active: AutoSwitchAccount?) -> String {
        guard let active, let usage = active.usage else { return "Claude Code's account has no usage" }
        let limits = AutoSwitchPlanner.effectiveThresholds(for: active.id, base: settings.thresholds, lock: lock)
        let over = [("5h", usage.fiveHour.utilization, limits.fiveHour),
                    ("7d", usage.sevenDay.utilization, limits.sevenDay)]
            .filter { $0.1 >= $0.2 }
            .map { "\($0.0) \(Self.percent($0.1)) >= \(Self.percent($0.2))" }
        return over.isEmpty ? Self.usageText(active) : over.joined(separator: ", ")
    }

    private static func usageText(_ account: AutoSwitchAccount) -> String {
        "5h \(percent(account.fiveHour)), 7d \(percent(account.sevenDay))"
    }

    private static func percent(_ value: Double) -> String { "\(Int(value.rounded()))%" }

    private static func isActiveLoginError(_ error: Error) -> Bool {
        switch error as? SwitchError {
        case .activeAccountNotInDashboard?, .activeAccountUnknown?: return true
        default: return false
        }
    }

    private static func describe(_ error: Error) -> String {
        switch error as? SwitchError {
        case .notCaptured?, .loginExpired?:
            return "it needs a browser sign-in first"
        case .activeAccountNotInDashboard?, .activeAccountUnknown?:
            return "Claude Code's current login is not a dashboard account"
        case let switchError?:
            return switchError.logName
        case nil:
            return error.localizedDescription
        }
    }
}
