import Foundation
import Combine
import SwiftUI

struct AccountUsageState: Identifiable {
    let id: UUID
    var account: Account
    var usage: UsageData?
    var isLoading: Bool = false
    var error: String?
    var burnRates: BurnRates?
}

@MainActor
final class DashboardViewModel: ObservableObject {
    @Published var accountStates: [AccountUsageState] = []
    @Published var isRefreshing = false
    @Published var activeClaudeCodeEmail: String?

    @Published var autoRefreshEnabled: Bool {
        didSet { UserDefaults.standard.set(autoRefreshEnabled, forKey: "autoRefreshEnabled"); scheduleAutoRefresh() }
    }
    @Published var autoRefreshMinutes: Int {
        didSet { UserDefaults.standard.set(autoRefreshMinutes, forKey: "autoRefreshMinutes"); scheduleAutoRefresh() }
    }

    enum NavigationDestination: Equatable {
        case dashboard
        case accountDetail(UUID, UsageWindow)
        case overview
    }

    @Published var navigation: NavigationDestination = .dashboard
    @Published var isPresentingSettings = false
    @Published var lastLogsUpdatedAt: Date = .distantPast

    let accountStore: AccountStore
    private let apiService: UsageAPIService
    private let ccDetector: ClaudeCodeAccountDetector
    private var cancellables = Set<AnyCancellable>()
    private var autoRefreshTask: Task<Void, Never>?
    private var resetMonitorTask: Task<Void, Never>?
    /// Accounts already pinged for the current "circle not started" episode. An account
    /// is cleared once both its windows report a reset again, so the next reset re-fires
    /// the command exactly once instead of on every refresh while `resetsAt` stays nil.
    private var pingedAccounts: Set<UUID> = []
    private let burnRateTracker: BurnRateTracker
    let logStore: UsageLogStore
    let commandLogStore: CommandLogStore
    let commandRunner: CommandRunner

    init(
        accountStore: AccountStore = AccountStore(),
        apiService: UsageAPIService = UsageAPIService(),
        logStore: UsageLogStore? = nil,
        commandLogStore: CommandLogStore? = nil,
        ccDetector: ClaudeCodeAccountDetector = ClaudeCodeAccountDetector()
    ) {
        self.autoRefreshEnabled = UserDefaults.standard.object(forKey: "autoRefreshEnabled") as? Bool ?? true
        self.autoRefreshMinutes = {
            let val = UserDefaults.standard.integer(forKey: "autoRefreshMinutes")
            return val > 0 ? val : 5
        }()
        self.accountStore = accountStore
        self.apiService = apiService
        self.ccDetector = ccDetector
        let store = logStore ?? UsageLogStore()
        self.logStore = store
        self.burnRateTracker = BurnRateTracker(logStore: store)
        let cmdStore = commandLogStore ?? CommandLogStore()
        self.commandLogStore = cmdStore
        self.commandRunner = CommandRunner(store: cmdStore)
        self.activeClaudeCodeEmail = ccDetector.activeEmail()

        // Cleanup old logs on launch
        Task {
            await store.deleteOlderThan(Date().addingTimeInterval(-90 * 24 * 3600))
        }

        accountStore.$accounts
            .receive(on: DispatchQueue.main)
            .sink { [weak self] accounts in
                self?.syncStates(with: accounts)
            }
            .store(in: &cancellables)

        scheduleAutoRefresh()
        startResetMonitor()

        // Auto-load data on launch
        Task { await self.refreshAll() }
    }

    private func scheduleAutoRefresh() {
        autoRefreshTask?.cancel()
        autoRefreshTask = nil
        guard autoRefreshEnabled else { return }
        let interval = UInt64(autoRefreshMinutes) * 60 * 1_000_000_000
        autoRefreshTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: interval)
                guard !Task.isCancelled else { break }
                await self?.refreshAll()
            }
        }
    }

    // MARK: - Reset Monitor

    private func startResetMonitor() {
        resetMonitorTask?.cancel()
        resetMonitorTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 5_000_000_000)
                guard !Task.isCancelled else { break }
                await self?.checkResetTriggers()
            }
        }
    }

    /// Once a window's reset time has passed, the next API fetch reports `resets_at:
    /// null`. Refresh promptly so `refreshAll` can fire the saved command to restart
    /// the circle within seconds, instead of waiting for the next auto-refresh. The
    /// 10s grace avoids reacting to the sub-second jitter `resets_at` shows right at
    /// the boundary. Firing and de-duplication live in `refreshAll`, not here, so the
    /// command can never fire twice for one reset.
    private func checkResetTriggers() async {
        let threshold = Date().addingTimeInterval(-10)
        let circlePassed = accountStates.contains { state in
            guard state.account.status == .active, let usage = state.usage else { return false }
            let five = usage.fiveHour.resetsAt.map { $0 <= threshold } ?? false
            let seven = usage.sevenDay.resetsAt.map { $0 <= threshold } ?? false
            return five || seven
        }
        if circlePassed {
            await refreshAll()
        }
    }

    /// The saved command exists to (re)start a reset circle. The true signal is a nil
    /// `resetsAt` on either tracked window (5h / 7d): Claude returns `resets_at: null`
    /// after a window resets and until its next first request, which is exactly the
    /// "circle not started" state. Utilization is the wrong signal — it sits at 0 for
    /// any idle-but-running window, so the old `utilization == 0` check fired on every
    /// refresh for unused accounts. A failed fetch (usage == nil) does NOT fire: we
    /// can't read the window state, and a network blip must not trigger a ping.
    static func shouldRunSavedCommand(for usage: UsageData?) -> Bool {
        guard let usage else { return false }
        return usage.fiveHour.resetsAt == nil || usage.sevenDay.resetsAt == nil
    }

    // MARK: - Refresh

    func refreshAll() async {
        isRefreshing = true
        defer { isRefreshing = false }
        activeClaudeCodeEmail = ccDetector.activeEmail()

        // Saved commands to run for accounts whose refresh produced no 5h/7d usage.
        var autoCommands: [(accountId: UUID, command: String)] = []

        await withTaskGroup(of: (UUID, UsageData?, String?, AccountPlan?).self) { group in
            for state in accountStates where state.account.status != .expired {
                let account = state.account
                guard let sessionKey = accountStore.loadSessionKey(for: account.id),
                      let orgId = account.orgId else {
                    continue
                }

                group.addTask { [apiService, accountStore] in
                    do {
                        let (usage, newKey) = try await Self.fetchWithRetry {
                            try await apiService.fetchFullUsage(orgId: orgId, sessionKey: sessionKey)
                        }
                        if let newKey {
                            await MainActor.run { accountStore.saveSessionKey(newKey, for: account.id) }
                        }
                        // Plan tier comes only from org capabilities (the usage endpoint
                        // has no reliable plan signal). Best-effort: a nil result leaves
                        // the stored plan unchanged in the update step below.
                        let planHint = (try? await apiService.fetchOrganizations(sessionKey: sessionKey))?
                            .first(where: { $0.uuid == orgId })?.planHint
                        return (account.id, usage, nil, planHint)
                    } catch UsageAPIError.authExpired {
                        return (account.id, nil, "expired", nil)
                    } catch is DecodingError {
                        return (account.id, nil, "Temporary read error. Try refreshing.", nil)
                    } catch {
                        return (account.id, nil, error.localizedDescription, nil)
                    }
                }
            }

            for await (accountId, usage, error, planHint) in group {
                if let index = accountStates.firstIndex(where: { $0.id == accountId }) {
                    accountStates[index].usage = usage ?? accountStates[index].usage
                    accountStates[index].error = error

                    // Run the account's saved command once when a window's circle has
                    // reset (resets_at == nil). Evaluate against the stored usage so a
                    // failed fetch keeps the prior state instead of re-arming. The ping
                    // fills resets_at, ending the nil episode; `pingedAccounts` blocks a
                    // re-fire until both circles are present again (re-arm below).
                    let acctId = accountStates[index].account.id
                    if Self.shouldRunSavedCommand(for: accountStates[index].usage) {
                        let cmdKey = "runCommand_\(acctId.uuidString)"
                        if let cmd = UserDefaults.standard.string(forKey: cmdKey), !cmd.isEmpty,
                           !pingedAccounts.contains(acctId) {
                            pingedAccounts.insert(acctId)
                            autoCommands.append((acctId, cmd))
                        }
                    } else {
                        pingedAccounts.remove(acctId)
                    }

                    if error == "expired" {
                        var account = accountStates[index].account
                        account.status = .expired
                        accountStore.updateAccount(account)
                    } else if error == nil {
                        var account = accountStates[index].account
                        account.status = .active
                        account.lastSynced = Date()
                        if let planHint, account.plan != planHint {
                            account.plan = planHint
                        }
                        accountStore.updateAccount(account)
                    }

                    // Record burn rates
                    if let currentUsage = accountStates[index].usage {
                        var rates = BurnRates()
                        rates.fiveHour = await burnRateTracker.record(
                            accountId: accountId, window: .fiveHour,
                            utilization: currentUsage.fiveHour.utilization,
                            resetsAt: currentUsage.fiveHour.resetsAt ?? Date().addingTimeInterval(18000)
                        )
                        rates.sevenDay = await burnRateTracker.record(
                            accountId: accountId, window: .sevenDay,
                            utilization: currentUsage.sevenDay.utilization,
                            resetsAt: currentUsage.sevenDay.resetsAt ?? Date().addingTimeInterval(604800)
                        )
                        if let fable = currentUsage.fable {
                            rates.fable = await burnRateTracker.record(
                                accountId: accountId, window: .fable,
                                utilization: fable.utilization,
                                resetsAt: fable.resetsAt ?? Date().addingTimeInterval(604800)
                            )
                        }
                        accountStates[index].burnRates = rates
                    }
                }
            }
        }

        // Fire the de-duplicated saved commands without blocking the refresh spinner.
        // Each fires once per reset (see pingedAccounts), so the command log no longer
        // fills with repeats while a window sits idle.
        if !autoCommands.isEmpty {
            let commands = autoCommands
            // Copy to a local so the detached task doesn't capture @MainActor self.
            let runner = commandRunner
            Task.detached {
                await withTaskGroup(of: Void.self) { group in
                    for item in commands {
                        group.addTask {
                            await runner.run(command: item.command, accountId: item.accountId, trigger: .autoReset)
                        }
                    }
                }
            }
        }

        // Sort: pinned > (active Claude Code if no pin) > burn rate
        sortStates()
        lastLogsUpdatedAt = Date()
    }

    func resyncAccount(_ accountId: UUID) async {
        guard let account = accountStore.accounts.first(where: { $0.id == accountId }) else { return }

        let cookies = BrowserCookieService.extractCookies(for: account.chromeProfilePath, browser: account.browser)

        guard let sessionKey = cookies.sessionKey else {
            // Re-sync failed — keep expired status, update error message
            if let index = accountStates.firstIndex(where: { $0.id == accountId }) {
                let profileName = account.chromeProfileName ?? account.chromeProfilePath
                accountStates[index].error = "Re-sync failed. Open \(account.browser.displayName) profile \"\(profileName)\" and login to claude.ai first."
            }
            return
        }

        var updated = account
        updated.sessionKey = CryptoService.encrypt(sessionKey) ?? sessionKey
        updated.status = .active
        if let orgId = cookies.orgId {
            updated.orgId = orgId
        }
        accountStore.updateAccount(updated)

        // Auto-refresh this account after re-sync
        Task {
            await refreshAll()
        }
    }

    // MARK: - Active Claude Code Account

    func isActiveClaudeCodeAccount(_ state: AccountUsageState) -> Bool {
        guard let active = activeClaudeCodeEmail else { return false }
        return state.account.email == active
    }

    // MARK: - Pin

    func togglePin(for accountId: UUID) {
        let wasPinned = accountStore.accounts.first(where: { $0.id == accountId })?.isPinned ?? false

        // Unpin all accounts
        for account in accountStore.accounts where account.isPinned {
            var updated = account
            updated.isPinned = false
            accountStore.updateAccount(updated)
        }

        // If it wasn't pinned before, pin it now
        if !wasPinned, var target = accountStore.accounts.first(where: { $0.id == accountId }) {
            target.isPinned = true
            accountStore.updateAccount(target)
        }
    }

    // MARK: - Menubar Label

    private var menuBarSource: UsageLimit? {
        if let pinned = accountStates.first(where: { $0.account.isPinned }),
           let usage = pinned.usage {
            return usage.fiveHour
        }
        return accountStates.first { $0.usage != nil }?.usage?.fiveHour
    }

    var menuBarPercentText: String {
        guard let limit = menuBarSource else { return "--" }
        return "\(Int(limit.utilization))%"
    }

    var menuBarTimeText: String? {
        guard let limit = menuBarSource,
              let reset = limit.resetsAt else { return nil }
        let remaining = reset.timeIntervalSinceNow
        guard remaining > 0 else { return nil }
        let h = Int(remaining) / 3600
        let m = (Int(remaining) % 3600) / 60
        return "\(h)h\(String(format: "%02d", m))m"
    }

    // MARK: - Color

    static func usageColor(for utilization: Double) -> Color {
        // HSB interpolation: hue 120° (green) → 0° (red)
        let hue = max(0, min(120, 120 * (1 - utilization / 100))) / 360
        return Color(hue: hue, saturation: 0.7, brightness: 0.85)
    }

    /// Burn rate = utilization / time remaining. Higher = consuming faster = shown first.
    static func burnRate(for state: AccountUsageState) -> Double {
        guard state.account.status == .active,
              let usage = state.usage else {
            return -1  // expired/error/no-data go to bottom
        }

        let utilization = usage.fiveHour.utilization
        let timeRemaining: TimeInterval
        if let resetsAt = usage.fiveHour.resetsAt {
            timeRemaining = max(resetsAt.timeIntervalSinceNow, 60)
        } else {
            timeRemaining = 18000  // assume full 5h if no reset time
        }

        return utilization / timeRemaining
    }

    // MARK: - Private

    private func syncStates(with accounts: [Account]) {
        let existingMap = Dictionary(uniqueKeysWithValues: accountStates.map { ($0.id, $0) })
        accountStates = accounts.map { account in
            if let existing = existingMap[account.id] {
                return AccountUsageState(
                    id: account.id,
                    account: account,
                    usage: existing.usage,
                    isLoading: existing.isLoading,
                    error: existing.error,
                    burnRates: existing.burnRates
                )
            }
            return AccountUsageState(id: account.id, account: account)
        }
        // Sort: pinned > (active Claude Code if no pin) > burn rate
        sortStates()
    }

    private static func fetchWithRetry<T>(
        maxAttempts: Int = 3,
        delay: TimeInterval = 1.0,
        operation: () async throws -> T
    ) async throws -> T {
        var lastError: Error?
        for attempt in 0..<maxAttempts {
            do {
                return try await operation()
            } catch UsageAPIError.authExpired {
                throw UsageAPIError.authExpired
            } catch {
                lastError = error
                if attempt < maxAttempts - 1 {
                    try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
                }
            }
        }
        throw lastError!
    }

    func sortStates() {
        let anyPinned = accountStates.contains { $0.account.isPinned }
        accountStates.sort { lhs, rhs in
            // 1. Pinned first
            if lhs.account.isPinned != rhs.account.isPinned {
                return lhs.account.isPinned
            }
            // 2. If no account is pinned anywhere, active Claude Code account next
            if !anyPinned {
                let lhsActive = isActiveClaudeCodeAccount(lhs)
                let rhsActive = isActiveClaudeCodeAccount(rhs)
                if lhsActive != rhsActive { return lhsActive }
            }
            // 3. Burn rate (unchanged)
            return DashboardViewModel.burnRate(for: lhs) > DashboardViewModel.burnRate(for: rhs)
        }
    }
}
