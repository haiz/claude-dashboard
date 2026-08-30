import Foundation

enum HelperAccountStore {

    private static let suiteName = "com.claude-dashboard.app"
    private static let storageKey = "claude-dashboard.accounts"

    static func loadAccounts() -> [Account] {
        guard let defaults = UserDefaults(suiteName: suiteName),
              let data = defaults.data(forKey: storageKey),
              let accounts = try? JSONDecoder().decode([Account].self, from: data) else {
            return []
        }
        return accounts
    }

    static func saveAccounts(_ accounts: [Account]) {
        guard let defaults = UserDefaults(suiteName: suiteName),
              let data = try? JSONEncoder().encode(accounts) else {
            return
        }
        defaults.set(data, forKey: storageKey)
    }
}
