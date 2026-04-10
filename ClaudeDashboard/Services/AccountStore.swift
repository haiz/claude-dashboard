import Foundation
import Combine

final class AccountStore: ObservableObject {
    @Published private(set) var accounts: [Account] = []

    private let defaults: UserDefaults
    private let storageKey = "claude-dashboard.accounts"

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        self.accounts = loadAccounts()
    }

    func addAccount(_ account: Account) {
        accounts.append(account)
        persist()
    }

    func removeAccount(id: UUID) {
        accounts.removeAll { $0.id == id }
        KeychainService.delete(key: KeychainService.sessionKey(for: id))
        persist()
    }

    func updateAccount(_ account: Account) {
        guard let index = accounts.firstIndex(where: { $0.id == account.id }) else { return }
        accounts[index] = account
        persist()
    }

    func saveSessionKey(_ key: String, for accountId: UUID) {
        KeychainService.save(key: KeychainService.sessionKey(for: accountId), value: key)
    }

    func loadSessionKey(for accountId: UUID) -> String? {
        KeychainService.load(key: KeychainService.sessionKey(for: accountId))
    }

    // MARK: - Persistence

    private func persist() {
        guard let data = try? JSONEncoder().encode(accounts) else { return }
        defaults.set(data, forKey: storageKey)
    }

    private func loadAccounts() -> [Account] {
        guard let data = defaults.data(forKey: storageKey),
              let accounts = try? JSONDecoder().decode([Account].self, from: data) else {
            return []
        }
        return accounts
    }
}
