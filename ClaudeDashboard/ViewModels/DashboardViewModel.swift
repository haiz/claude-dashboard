import Foundation
import Combine
import SwiftUI

struct AccountUsageState: Identifiable {
    let id: UUID
    let account: Account
    var usage: UsageData?
    var isLoading: Bool = false
    var error: String?
}

@MainActor
final class DashboardViewModel: ObservableObject {
    @Published var accountStates: [AccountUsageState] = []
    @Published var isRefreshing = false

    let accountStore: AccountStore
    private let apiService: UsageAPIService
    private var cancellables = Set<AnyCancellable>()

    init(accountStore: AccountStore = AccountStore(), apiService: UsageAPIService = UsageAPIService()) {
        self.accountStore = accountStore
        self.apiService = apiService

        accountStore.$accounts
            .receive(on: DispatchQueue.main)
            .sink { [weak self] accounts in
                self?.syncStates(with: accounts)
            }
            .store(in: &cancellables)
    }

    // MARK: - Refresh

    func refreshAll() async {
        isRefreshing = true
        defer { isRefreshing = false }

        await withTaskGroup(of: (UUID, UsageData?, String?).self) { group in
            for state in accountStates where state.account.status != .expired {
                let account = state.account
                guard let sessionKey = accountStore.loadSessionKey(for: account.id),
                      let orgId = account.orgId else {
                    continue
                }

                group.addTask { [apiService, accountStore] in
                    do {
                        let result = try await apiService.fetchUsage(orgId: orgId, sessionKey: sessionKey)
                        if let newKey = result.newSessionKey {
                            accountStore.saveSessionKey(newKey, for: account.id)
                        }
                        return (account.id, result.usage, nil)
                    } catch UsageAPIError.authExpired {
                        return (account.id, nil, "expired")
                    } catch {
                        return (account.id, nil, error.localizedDescription)
                    }
                }
            }

            for await (accountId, usage, error) in group {
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
                        accountStore.updateAccount(account)
                    }
                }
            }
        }
    }

    func resyncAccount(_ accountId: UUID) {
        guard let account = accountStore.accounts.first(where: { $0.id == accountId }) else { return }

        let cookies = ChromeCookieService.extractCookies(for: account.chromeProfilePath)
        guard let sessionKey = cookies.sessionKey else { return }

        accountStore.saveSessionKey(sessionKey, for: accountId)

        var updated = account
        updated.status = .active
        if let orgId = cookies.orgId {
            updated.orgId = orgId
        }
        accountStore.updateAccount(updated)
    }

    // MARK: - Menubar Label

    var menuBarLabel: String {
        guard let highest = accountStates
            .compactMap({ $0.usage?.fiveHour })
            .max(by: { $0.utilization < $1.utilization }) else {
            return "--"
        }

        let pct = Int(highest.utilization)
        if let reset = highest.resetsAt {
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
                    error: existing.error
                )
            }
            return AccountUsageState(id: account.id, account: account)
        }
    }
}
