import Foundation

/// What a ping needs to reach claude.ai for one account.
struct PingTarget: Equatable, Sendable {
    let accountId: UUID
    let orgId: String
    let sessionKey: String
}

/// Runs pings and decides auto pings. All state is in memory except the
/// Auto Ping toggle (`autoPingEnabled`, default off).
///
/// Auto-ping rule (spec, "Auto Ping"): a successfully refreshed `.active`
/// account whose 5h or 7d window is not running (`resets_at == nil`) is pinged,
/// once. The latch holds until both windows run again; a failed auto ping may
/// retry after `failureCooldown`.
///
///     if controller.claimAutoPing(accountId: id, status: .active, freshUsage: usage) {
///         await controller.ping(target, trigger: .auto)
///     }
@MainActor
final class PingController: ObservableObject {
    static let enabledKey = "autoPingEnabled"
    static let failureCooldown: TimeInterval = 10 * 60

    @Published var isAutoPingEnabled: Bool {
        didSet { defaults.set(isAutoPingEnabled, forKey: Self.enabledKey) }
    }
    /// Accounts with a ping running; the account page disables its button for them.
    @Published private(set) var inFlight: Set<UUID> = []
    /// Each account's latest ping, from this session or loaded from the log.
    @Published private(set) var latest: [UUID: PingLogEntry] = [:]

    let logStore: PingLogStore
    private let service: Pinging
    private let defaults: UserDefaults
    private let now: () -> Date
    /// Pinged for the current "window not running" episode.
    private var latched: Set<UUID> = []
    /// A latched account whose auto ping failed may be claimed again from this time on.
    private var retryAfter: [UUID: Date] = [:]

    init(service: Pinging, logStore: PingLogStore, defaults: UserDefaults,
         now: @escaping () -> Date = Date.init) {
        self.service = service
        self.logStore = logStore
        self.defaults = defaults
        self.now = now
        self.isAutoPingEnabled = defaults.bool(forKey: Self.enabledKey)
    }

    static func isWindowNotRunning(_ usage: UsageData) -> Bool {
        usage.fiveHour.resetsAt == nil || usage.sevenDay.resetsAt == nil
    }

    /// True when `accountId` should be auto-pinged now; the account is latched as
    /// claimed. `freshUsage` is this refresh's fetch, nil when it failed: an
    /// unreadable window state never pings.
    func claimAutoPing(accountId: UUID, status: AccountStatus, freshUsage: UsageData?) -> Bool {
        guard let usage = freshUsage else { return false }
        guard Self.isWindowNotRunning(usage) else {
            latched.remove(accountId)
            retryAfter[accountId] = nil
            return false
        }
        guard isAutoPingEnabled, status == .active, !inFlight.contains(accountId) else { return false }
        if latched.contains(accountId) {
            guard let until = retryAfter[accountId], now() >= until else { return false }
        }
        latched.insert(accountId)
        retryAfter[accountId] = nil
        return true
    }

    /// Pings and logs one account. nil when a ping for it is already running.
    @discardableResult
    func ping(_ target: PingTarget, trigger: PingTrigger) async -> PingResult? {
        let id = target.accountId
        guard !inFlight.contains(id) else { return nil }
        inFlight.insert(id)
        defer { inFlight.remove(id) }

        let started = now()
        let result = await service.ping(orgId: target.orgId, sessionKey: target.sessionKey)
        let finished = now()

        if result.outcome == .ok {
            // A manual ping started the window too; do not auto-ping it again.
            latched.insert(id)
            retryAfter[id] = nil
        } else if trigger == .auto {
            retryAfter[id] = finished.addingTimeInterval(Self.failureCooldown)
        }

        let rowId = await logStore.insert(accountId: id, trigger: trigger, startedAt: started,
                                          finishedAt: finished, result: result)
        latest[id] = PingLogEntry(id: rowId, accountId: id, trigger: trigger, startedAt: started,
                                  finishedAt: finished, outcome: result.outcome, detail: result.detail)
        return result
    }

    func loadLatest(for accountId: UUID) async {
        if let entry = await logStore.latest(for: accountId) {
            latest[accountId] = entry
        }
    }
}
