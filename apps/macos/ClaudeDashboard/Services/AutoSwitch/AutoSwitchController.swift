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
/// planner must not: the lock (in memory only), the 10-minute failure cooldown and the
/// footer event. The switch itself is the caller's closure, so this never touches the
/// Keychain and never opens a browser.
///
///     let controller = AutoSwitchController(settings: settings)
///     await controller.evaluate(accounts) { id in try await vm.performAutoSwitch(to: id) }
@MainActor
final class AutoSwitchController: ObservableObject {
    static let failureCooldown: TimeInterval = 600

    let settings: AutoSwitchSettings
    @Published private(set) var lastEvent: AutoSwitchEvent?
    private(set) var lock: AutoSwitchLock?
    private var cooldownUntil: [UUID: Date] = [:]
    /// Set when a switch fails because of Claude Code's current login, not the target:
    /// every target would fail the same way, so auto switch pauses as a whole.
    private var pausedUntil: Date?
    private let now: () -> Date
    private let pickIndex: (Int) -> Int

    init(settings: AutoSwitchSettings,
         now: @escaping () -> Date = Date.init,
         pickIndex: @escaping (Int) -> Int = { Int.random(in: 0..<$0) }) {
        self.settings = settings
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
        // A tick on a removed or now-active account has nothing left to wait for.
        if let priority = settings.priority,
           accounts.first(where: { $0.id == priority.accountId })?.isActive ?? true {
            settings.setPriority(nil)
        }
        guard settings.isEnabled else { return nil }

        let t = now()
        if let pausedUntil, pausedUntil > t { return nil }
        pausedUntil = nil
        cooldownUntil = cooldownUntil.filter { $0.value > t }
        lock = AutoSwitchPlanner.updatedLock(lock, active: accounts.first(where: \.isActive))
        let snapshot = AutoSwitchSnapshot(accounts: accounts, thresholds: settings.thresholds, lock: lock,
                                          priority: settings.priority, excluded: Set(cooldownUntil.keys), now: t)

        switch AutoSwitchPlanner.decide(snapshot, pickIndex: pickIndex) {
        case .stay(.noCandidate):
            lastEvent = AutoSwitchEvent(kind: .noCandidate, accountName: nil, date: t)
            return nil
        case .stay:
            return nil
        case .switchTo(let id, let reason):
            let target = accounts.first { $0.id == id }
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
                return nil
            }
            lock = target?.usage.flatMap {
                AutoSwitchPlanner.makeLock(for: id, usage: $0, thresholds: settings.thresholds)
            }
            if settings.priority?.accountId == id { settings.setPriority(nil) }
            lastEvent = AutoSwitchEvent(kind: .switched(reason), accountName: target?.displayName, date: t)
            return id
        }
    }

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
