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

/// What applying a pasted session key did. The view maps this to a message; the
/// key itself never appears in any of them.
enum ManualKeyOutcome: Equatable {
    case added(name: String)
    case updated(name: String)
    /// Repaired, but the account has no organisation with chat access, so usage
    /// will not update. The key is still saved — see `resyncCore`.
    case updatedWithNoChatOrg(name: String)
    case rejectedNoChatOrg
    case keyNotAccepted
    case emptyKey
}

extension ManualKeyOutcome {
    var message: String {
        switch self {
        case .added(let name):
            return "Added \(name)."
        case .updated(let name):
            return "Updated the session key for \(name)."
        case .updatedWithNoChatOrg(let name):
            return "Updated the session key for \(name), but that account has no organization with chat access, so usage will not update."
        case .rejectedNoChatOrg:
            return "That account has no organization with chat access."
        case .keyNotAccepted:
            return "That session key was not accepted. It may have expired, or been copied incompletely."
        case .emptyKey:
            return "Paste a session key first."
        }
    }

    var isFailure: Bool {
        switch self {
        case .added, .updated, .updatedWithNoChatOrg: return false
        case .rejectedNoChatOrg, .keyNotAccepted, .emptyKey: return true
        }
    }
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
    /// Non-nil while a "Re-sync All" pass runs: (accounts re-synced so far, total).
    @Published private(set) var resyncAllProgress: (done: Int, total: Int)?
    @Published var isPresentingSettings = false
    @Published var lastLogsUpdatedAt: Date = .distantPast

    let accountStore: AccountStore
    private let apiService: UsageAPIService
    private let ccDetector: ClaudeCodeAccountDetector
    /// Reads the browser cookies for one stored account. Injected so `resyncAccount`
    /// can be driven in tests without a real Chromium cookie DB on disk.
    private let cookieProvider: (String, Browser) -> ChromeCookieResult
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
        ccDetector: ClaudeCodeAccountDetector = ClaudeCodeAccountDetector(),
        cookieProvider: @escaping (String, Browser) -> ChromeCookieResult
            = BrowserCookieService.extractCookies(for:browser:)
    ) {
        self.autoRefreshEnabled = UserDefaults.standard.object(forKey: "autoRefreshEnabled") as? Bool ?? true
        self.autoRefreshMinutes = {
            let val = UserDefaults.standard.integer(forKey: "autoRefreshMinutes")
            return val > 0 ? val : 5
        }()
        self.accountStore = accountStore
        self.apiService = apiService
        self.ccDetector = ccDetector
        self.cookieProvider = cookieProvider
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

    /// `only` limits the pass to those accounts; `nil` refreshes the whole fleet.
    /// A scoped pass is what lets a re-sync refresh the card it touched without
    /// overwriting the error message another card is showing.
    func refreshAll(only ids: Set<UUID>? = nil) async {
        isRefreshing = true
        defer { isRefreshing = false }
        // The store publishes into `accountStates` asynchronously (`receive(on: .main)`),
        // so a caller that just wrote an account and refreshed in the same turn would
        // read its own stale copy here: a re-synced account would still look `.expired`
        // and be filtered out, a freshly added one would be missing entirely.
        syncStates(with: accountStore.accounts)
        activeClaudeCodeEmail = ccDetector.activeEmail()

        // Saved commands to run for accounts whose refresh produced no 5h/7d usage.
        var autoCommands: [(accountId: UUID, command: String)] = []

        await withTaskGroup(of: (UUID, UsageData?, String?, AccountPlan?, AccountInfo?).self) { group in
            for state in accountStates
            where state.account.status != .expired && (ids?.contains(state.id) ?? true) {
                let account = state.account
                guard let sessionKey = accountStore.loadSessionKey(for: account.id),
                      let orgId = account.orgId else {
                    continue
                }

                let needsIdentity = account.accountUuid == nil

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
                        // Backfill: records written before accountUuid existed
                        // get it on the first successful refresh. Best-effort;
                        // a failure leaves the record as it was.
                        var info: AccountInfo? = nil
                        if needsIdentity {
                            info = try? await apiService.fetchAccount(sessionKey: sessionKey)
                        }
                        return (account.id, usage, nil, planHint, info)
                    } catch UsageAPIError.authExpired {
                        return (account.id, nil, "expired", nil, nil)
                    } catch is DecodingError {
                        return (account.id, nil, "Temporary read error. Try refreshing.", nil, nil)
                    } catch {
                        return (account.id, nil, error.localizedDescription, nil, nil)
                    }
                }
            }

            for await (accountId, usage, error, planHint, info) in group {
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
                        // `contract/cases/plan-refresh.json` — the same rule the
                        // helpers' `sync` applies to an already-stored account.
                        if let newPlan = UsageAPIService.refreshedPlan(
                            stored: account.plan, hint: planHint) {
                            account.plan = newPlan
                        }
                        // Identity backfill. Never touches orgId: re-resolving a
                        // stored account's org would silently rewrite a field the
                        // user may have a working value in.
                        if let info {
                            if account.accountUuid == nil { account.accountUuid = info.uuid }
                            if account.email == nil { account.email = info.email }
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

    /// Re-reads one account's browser cookies and writes the new session key, the
    /// identity backfill and the resolved org. Returns `true` when the record is
    /// ready for a usage refresh; on `false` the reason is already on the card as
    /// an error message. Runs no refresh itself, so a caller re-syncing several
    /// accounts pays for one refresh pass instead of one per account.
    private func resyncCore(_ accountId: UUID) async -> Bool {
        guard let account = accountStore.accounts.first(where: { $0.id == accountId }) else { return false }

        let cookies = cookieProvider(account.chromeProfilePath, account.browser)

        guard let sessionKey = cookies.sessionKey else {
            // Re-sync failed — keep expired status, update error message
            let profileName = account.chromeProfileName ?? account.chromeProfilePath
            setError("Re-sync failed. Open \(account.browser.displayName) profile \"\(profileName)\" and login to claude.ai first.",
                     for: accountId)
            return false
        }

        // Who the profile is signed in as now, and which orgs that session may be
        // polled against. A failure here is not fatal — see the org write below.
        let info = try? await apiService.fetchAccount(sessionKey: sessionKey)

        // The profile has been switched to a different Claude account. Writing its
        // session key onto this record would point two cards at one account, the
        // corruption the dedupe rule exists to prevent. A legacy record (no
        // accountUuid) has nothing to compare against and is backfilled instead.
        if let info, let storedUuid = account.accountUuid, info.uuid != storedUuid {
            setError("Re-sync stopped. That browser profile is now signed in to a different Claude account.",
                     for: accountId)
            return false
        }

        var updated = account
        updated.sessionKey = CryptoService.encrypt(sessionKey) ?? sessionKey
        updated.status = .active

        // orgId follows the org-selection rule (contract/cases/org-selection.json),
        // never the raw cookie: `lastActiveOrg` may name an api-only org, or one this
        // account is not a member of, and writing it blind overwrites an orgId that
        // was resolved correctly. When /api/account is unreachable the stored orgId is
        // left untouched — the new key is still worth saving, and `.active` is what
        // lets `refreshAll` (which skips `.expired`) retry at all.
        var orgUnresolved = false
        if let info {
            if updated.accountUuid == nil { updated.accountUuid = info.uuid }
            if updated.email == nil { updated.email = info.email }
            if let orgId = AccountIdentity.resolveOrgId(
                lastActiveOrg: cookies.orgId, memberships: info.memberships) {
                updated.orgId = orgId
            } else {
                orgUnresolved = true
            }
        }
        accountStore.updateAccount(updated)

        guard !orgUnresolved else {
            // No chat org left to poll. Report it and skip the refresh, which would
            // clear the message a second later.
            setError("Re-sync incomplete. This account has no organization with chat access.",
                     for: accountId)
            return false
        }

        return true
    }

    func resyncAccount(_ accountId: UUID) async {
        guard await resyncCore(accountId) else { return }
        // Refresh just this card. A whole-fleet pass would re-fetch usage nobody
        // asked for and clear the error message any other card is showing.
        await refreshAll(only: [accountId])
    }

    /// Adds or repairs an account from a session key the user pasted.
    ///
    /// Which of the two happens is decided by the dedupe rule, not by the user:
    /// `contract/cases/dedupe.json` says whether this key names a stored account.
    /// What the repair branch may write is `contract/cases/manual-key.json`.
    func applyManualKey(_ raw: String) async -> ManualKeyOutcome {
        guard let sessionKey = ManualKeyInput.trimmedKey(from: raw) else { return .emptyKey }

        let stored = accountStore.accounts
        guard let candidate = await SessionCandidate.validate(
            sessionKey: sessionKey,
            against: stored.map(StoredIdentity.init),
            apiService: apiService
        ) else { return .keyNotAccepted }

        let target = candidate.duplicateIndex.map { StoredManualTarget(stored[$0]) }
        let decision = ManualKey.decision(
            stored: target,
            fetchedUuid: candidate.identity.uuid,
            fetchedEmail: candidate.identity.email,
            memberships: candidate.identity.memberships)

        switch decision {
        case .rejectNoChatOrg:
            return .rejectedNoChatOrg

        case .add(let orgId):
            let email = candidate.identity.email
            let name = email ?? "Account \(candidate.identity.uuid.prefix(8))"
            let plan = candidate.orgs?.first(where: { $0.uuid == orgId })?.planHint ?? .pro
            let account = Account(
                id: UUID(),
                name: name,
                email: email,
                chromeProfilePath: "",
                chromeProfileName: nil,
                orgId: orgId,
                accountUuid: candidate.identity.uuid,
                sessionKey: CryptoService.encrypt(sessionKey) ?? sessionKey,
                browser: .chrome,
                plan: plan,
                lastSynced: Date(),
                status: .active,
                source: .manual
            )
            accountStore.addAccount(account)
            await refreshAll(only: [account.id])
            return .added(name: name)

        case .repair(let writes):
            guard let index = candidate.duplicateIndex else { return .keyNotAccepted }
            var updated = stored[index]
            updated.sessionKey = CryptoService.encrypt(sessionKey) ?? sessionKey
            updated.status = .active
            updated.lastSynced = Date()
            if let orgId = writes.orgId { updated.orgId = orgId }
            if let uuid = writes.accountUuid { updated.accountUuid = uuid }
            if let email = writes.email { updated.email = email }
            // The plan is matched against the orgId as it stands *after* the
            // writes above: a stored nil that was just filled in would otherwise
            // match no org and leave the plan at its fallback forever.
            if let plan = UsageAPIService.refreshedPlan(for: updated, orgs: candidate.orgs) {
                updated.plan = plan
            }
            accountStore.updateAccount(updated)

            // Scoped for the same reason re-sync's is: an unscoped pass re-fetches
            // usage nobody asked for and wipes the message another card is showing.
            await refreshAll(only: [updated.id])

            let name = updated.email ?? updated.name
            return updated.orgId == nil
                ? .updatedWithNoChatOrg(name: name)
                : .updated(name: name)
        }
    }

    /// Re-syncs every stored account, then refreshes the ones that succeeded in a
    /// single pass. Looping over `resyncAccount` instead would run N refresh passes
    /// over the whole fleet (N x N usage fetches), and each pass would wipe the
    /// message left by an account that failed to re-sync.
    func resyncAll() async {
        // Set synchronously, before any await: a re-entrant call (double click on
        // the settings button) must see it and bail out here rather than start a
        // second overlapping pass.
        guard resyncAllProgress == nil else { return }
        let accounts = accountStore.accounts
        resyncAllProgress = (done: 0, total: accounts.count)
        defer { resyncAllProgress = nil }

        var refreshable: Set<UUID> = []
        for (index, account) in accounts.enumerated() {
            if await resyncCore(account.id) { refreshable.insert(account.id) }
            resyncAllProgress = (done: index + 1, total: accounts.count)
        }
        guard !refreshable.isEmpty else { return }
        await refreshAll(only: refreshable)
    }

    private func setError(_ message: String, for accountId: UUID) {
        guard let index = accountStates.firstIndex(where: { $0.id == accountId }) else { return }
        accountStates[index].error = message
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
