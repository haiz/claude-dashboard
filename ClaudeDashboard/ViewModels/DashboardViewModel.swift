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
        case accountDetail(UUID)
        case overview
    }

    @Published var navigation: NavigationDestination = .dashboard

    let accountStore: AccountStore
    private let apiService: UsageAPIService
    private let ccDetector: ClaudeCodeAccountDetector
    private var cancellables = Set<AnyCancellable>()
    private var autoRefreshTask: Task<Void, Never>?
    private let burnRateTracker: BurnRateTracker
    let logStore: UsageLogStore

    init(
        accountStore: AccountStore = AccountStore(),
        apiService: UsageAPIService = UsageAPIService(),
        logStore: UsageLogStore? = nil,
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

    // MARK: - Refresh

    func refreshAll() async {
        isRefreshing = true
        defer { isRefreshing = false }
        activeClaudeCodeEmail = ccDetector.activeEmail()

        await withTaskGroup(of: (UUID, UsageData?, String?, AccountPlan?).self) { group in
            for state in accountStates where state.account.status != .expired {
                let account = state.account
                guard let sessionKey = accountStore.loadSessionKey(for: account.id),
                      let orgId = account.orgId else {
                    continue
                }

                group.addTask { [apiService, accountStore] in
                    do {
                        let (usage, planHint, newKey) = try await apiService.fetchFullUsage(orgId: orgId, sessionKey: sessionKey)
                        if let newKey {
                            await MainActor.run { accountStore.saveSessionKey(newKey, for: account.id) }
                        }
                        return (account.id, usage, nil, planHint)
                    } catch UsageAPIError.authExpired {
                        return (account.id, nil, "expired", nil)
                    } catch {
                        return (account.id, nil, error.localizedDescription, nil)
                    }
                }
            }

            for await (accountId, usage, error, planHint) in group {
                if let index = accountStates.firstIndex(where: { $0.id == accountId }) {
                    accountStates[index].usage = usage ?? accountStates[index].usage
                    accountStates[index].error = error

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
                        if let sonnet = currentUsage.sevenDaySonnet {
                            rates.sonnet = await burnRateTracker.record(
                                accountId: accountId, window: .sonnet,
                                utilization: sonnet.utilization,
                                resetsAt: sonnet.resetsAt ?? Date().addingTimeInterval(604800)
                            )
                        }
                        accountStates[index].burnRates = rates
                    }
                }
            }
        }

        // Sort: pinned > (active Claude Code if no pin) > burn rate
        sortStates()
    }

    func resyncAccount(_ accountId: UUID) async {
        guard let account = accountStore.accounts.first(where: { $0.id == accountId }) else { return }

        let cookies = ChromeCookieService.extractCookies(for: account.chromeProfilePath)

        guard let sessionKey = cookies.sessionKey else {
            // Re-sync failed — keep expired status, update error message
            if let index = accountStates.firstIndex(where: { $0.id == accountId }) {
                let profileName = account.chromeProfileName ?? account.chromeProfilePath
                accountStates[index].error = "Re-sync failed. Open Chrome profile \"\(profileName)\" and login to claude.ai first."
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

    var menuBarLabel: String {
        // Prefer pinned account's usage, fallback to first sorted account
        // (sorted by active CC account → burn rate via sortStates)
        let source: UsageLimit? = {
            if let pinned = accountStates.first(where: { $0.account.isPinned }),
               let usage = pinned.usage {
                return usage.fiveHour
            }
            return accountStates.first { $0.usage != nil }?.usage?.fiveHour
        }()

        guard let limit = source else { return "--" }

        let pct = Int(limit.utilization)
        if let reset = limit.resetsAt {
            let remaining = reset.timeIntervalSinceNow
            if remaining > 0 {
                let h = Int(remaining) / 3600
                let m = (Int(remaining) % 3600) / 60
                return "\(pct)% \u{00B7} \(h)h\(String(format: "%02d", m))m"
            }
        }
        return "\(pct)%"
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
