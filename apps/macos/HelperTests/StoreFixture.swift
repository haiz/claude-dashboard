import Foundation

/// Builds and inspects a throwaway UserDefaults suite for the helper's account
/// store. Every suite name carries a UUID so two tests never share one, and
/// `destroy` removes the persistent domain so a run leaves nothing behind in
/// ~/Library/Preferences.
enum StoreFixture {

    static let storageKey = "claude-dashboard.accounts"

    static func makeSuiteName() -> String {
        "com.claude-dashboard.test.\(UUID().uuidString)"
    }

    static func seed(_ accounts: [Account], intoSuite suite: String) {
        guard let defaults = UserDefaults(suiteName: suite),
              let data = try? JSONEncoder().encode(accounts) else {
            fatalError("could not seed the fixture suite")
        }
        defaults.set(data, forKey: storageKey)
    }

    /// Reads what the *child process* wrote. `CFPreferencesAppSynchronize`
    /// first: preferences are served by cfprefsd and cached per process, so a
    /// parent that has already read this suite can otherwise serve a stale
    /// snapshot after the child writes to it.
    static func read(fromSuite suite: String) -> [Account] {
        CFPreferencesAppSynchronize(suite as CFString)
        guard let defaults = UserDefaults(suiteName: suite),
              let data = defaults.data(forKey: storageKey),
              let accounts = try? JSONDecoder().decode([Account].self, from: data) else {
            return []
        }
        return accounts
    }

    static func destroy(suite: String) {
        UserDefaults().removePersistentDomain(forName: suite)
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
    }

    /// A stored account with every field set explicitly. Defaults here are test
    /// convenience only; production code has none (see `Account.source`).
    static func account(
        id: UUID = UUID(),
        name: String,
        email: String? = nil,
        orgId: String?,
        accountUuid: String? = "acct-fixture",
        sessionKey: String? = "sk-fake-stored-key",
        plan: AccountPlan = .pro,
        status: AccountStatus = .active,
        source: AccountSource = .browser
    ) -> Account {
        Account(
            id: id,
            name: name,
            email: email ?? name,
            chromeProfilePath: "Default",
            chromeProfileName: nil,
            orgId: orgId,
            accountUuid: accountUuid,
            sessionKey: sessionKey,
            browser: .chrome,
            plan: plan,
            lastSynced: Date(timeIntervalSince1970: 1_000_000),
            status: status,
            source: source
        )
    }
}
