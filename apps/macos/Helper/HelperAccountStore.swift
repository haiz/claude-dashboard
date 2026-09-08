import Foundation

enum HelperAccountStore {

    /// Overrides the UserDefaults suite the store lives in, so a test can drive
    /// the real binary without writing into the user's own accounts. cfprefsd
    /// does not follow HOME, so this is the macOS counterpart of Linux's
    /// XDG_CONFIG_HOME. Platform detail, not contract.
    ///
    /// Defined in `AppDefaults` because the app target cannot see `Helper/`,
    /// and the two binaries have to name the same variable.
    static let suiteVariable = AppDefaults.suiteVariable

    private static let defaultSuiteName = "com.claude-dashboard.app"
    private static let storageKey = "claude-dashboard.accounts"

    /// `getenv` rather than `ProcessInfo.processInfo.environment`: Foundation
    /// may serve a cached snapshot of the environment, and the in-process test
    /// below calls `setenv` after the process has started.
    static func resolvedSuiteName() -> String {
        guard let raw = getenv(suiteVariable) else { return defaultSuiteName }
        return String(cString: raw)
    }

    static func loadAccounts() -> [Account] {
        guard let defaults = UserDefaults(suiteName: resolvedSuiteName()),
              let data = defaults.data(forKey: storageKey),
              let accounts = try? JSONDecoder().decode([Account].self, from: data) else {
            return []
        }
        return accounts
    }

    /// The load every writing path must use: three outcomes, not two
    /// (`contract/account-schema.md`'s "An unreadable store is not an empty
    /// store"). Bytes that will not decode are moved to a key of their own
    /// before the caller writes, and the returned name says which.
    /// `loadAccounts` above stays for read-only callers.
    static func loadAccountsForWrite() -> (accounts: [Account], quarantined: String?) {
        guard let defaults = UserDefaults(suiteName: resolvedSuiteName()) else {
            return ([], nil)
        }
        return AccountStoreQuarantine.decodeForWrite(from: defaults, key: storageKey)
    }

    static func saveAccounts(_ accounts: [Account]) {
        guard let defaults = UserDefaults(suiteName: resolvedSuiteName()),
              let data = try? JSONEncoder().encode(accounts) else {
            return
        }
        defaults.set(data, forKey: storageKey)
    }
}
