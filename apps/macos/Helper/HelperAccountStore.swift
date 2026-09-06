import Foundation

enum HelperAccountStore {

    /// Overrides the UserDefaults suite the store lives in, so a test can drive
    /// the real binary without writing into the user's own accounts. cfprefsd
    /// does not follow HOME, so this is the macOS counterpart of Linux's
    /// XDG_CONFIG_HOME. Platform detail, not contract.
    static let suiteVariable = "CLAUDE_DASHBOARD_DEFAULTS_SUITE"

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

    static func saveAccounts(_ accounts: [Account]) {
        guard let defaults = UserDefaults(suiteName: resolvedSuiteName()),
              let data = try? JSONEncoder().encode(accounts) else {
            return
        }
        defaults.set(data, forKey: storageKey)
    }
}
