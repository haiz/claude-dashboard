import Foundation

/// The `Account`-typed half of `StoreFixture`. It lives in this bundle, not in
/// `ClaudeDashboardTests/TestSupport/`, because `Account` comes from `Shared/`,
/// which only this bundle compiles directly -- the app bundle reaches the same
/// type through `@testable import ClaudeDashboard`, an import that does not
/// resolve here. The suite lifecycle both bundles share stays in TestSupport.
extension StoreFixture {

    static func seed(_ accounts: [Account], intoSuite suite: String) {
        guard let data = try? JSONEncoder().encode(accounts) else {
            fatalError("could not seed the fixture suite")
        }
        seedData(data, intoSuite: suite)
    }

    static func read(fromSuite suite: String) -> [Account] {
        guard let data = readData(fromSuite: suite),
              let accounts = try? JSONDecoder().decode([Account].self, from: data) else {
            return []
        }
        return accounts
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
