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
        persist()
    }

    func updateAccount(_ account: Account) {
        guard let index = accounts.firstIndex(where: { $0.id == account.id }) else { return }
        accounts[index] = account
        persist()
    }

    func saveSessionKey(_ key: String, for accountId: UUID) {
        guard let index = accounts.firstIndex(where: { $0.id == accountId }) else { return }
        accounts[index].sessionKey = CryptoService.encrypt(key) ?? key
        persist()
    }

    func loadSessionKey(for accountId: UUID) -> String? {
        guard let encrypted = accounts.first(where: { $0.id == accountId })?.sessionKey else { return nil }
        return CryptoService.decrypt(encrypted) ?? encrypted
    }

    // MARK: - Persistence

    private func persist() {
        guard let data = try? JSONEncoder().encode(accounts) else { return }
        defaults.set(data, forKey: storageKey)
    }

    /// Every mutation on this store calls `persist()`, so reading unparseable
    /// bytes as an empty list would write over them on the user's next add.
    /// `AccountStoreQuarantine` moves them aside first
    /// (`contract/account-schema.md`'s "An unreadable store is not an empty
    /// store"). Nothing surfaces this in the UI yet; what this call buys is
    /// that the bytes still exist to surface.
    private func loadAccounts() -> [Account] {
        AccountStoreQuarantine.decodeForWrite(from: defaults, key: storageKey).accounts
    }
}
