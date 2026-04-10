import XCTest
@testable import ClaudeDashboard

final class AccountStoreTests: XCTestCase {

    private var store: AccountStore!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        defaults = UserDefaults(suiteName: "com.claude-dashboard.tests")!
        defaults.removePersistentDomain(forName: "com.claude-dashboard.tests")
        store = AccountStore(defaults: defaults)
    }

    func testAddAccount() {
        let account = Account(
            id: UUID(),
            name: "Test Account",
            email: nil,
            chromeProfilePath: "Profile 1",
            chromeProfileName: nil,
            orgId: "org-123",
            plan: .max200,
            lastSynced: nil,
            status: .active
        )

        store.addAccount(account)

        XCTAssertEqual(store.accounts.count, 1)
        XCTAssertEqual(store.accounts.first?.name, "Test Account")
    }

    func testRemoveAccount() {
        let account = Account(
            id: UUID(),
            name: "Test",
            email: nil,
            chromeProfilePath: "Profile 1",
            chromeProfileName: nil,
            orgId: "org-123",
            plan: .pro,
            lastSynced: nil,
            status: .active
        )

        store.addAccount(account)
        XCTAssertEqual(store.accounts.count, 1)

        store.removeAccount(id: account.id)
        XCTAssertEqual(store.accounts.count, 0)
    }

    func testUpdateAccount() {
        var account = Account(
            id: UUID(),
            name: "Old Name",
            email: nil,
            chromeProfilePath: "Profile 1",
            chromeProfileName: nil,
            orgId: "org-123",
            plan: .max200,
            lastSynced: nil,
            status: .active
        )

        store.addAccount(account)
        account.name = "New Name"
        store.updateAccount(account)

        XCTAssertEqual(store.accounts.first?.name, "New Name")
    }

    func testPersistsAcrossInstances() {
        let account = Account(
            id: UUID(),
            name: "Persistent",
            email: nil,
            chromeProfilePath: "Profile 1",
            chromeProfileName: nil,
            orgId: "org-123",
            plan: .max200,
            lastSynced: nil,
            status: .active
        )

        store.addAccount(account)

        let store2 = AccountStore(defaults: defaults)
        XCTAssertEqual(store2.accounts.count, 1)
        XCTAssertEqual(store2.accounts.first?.name, "Persistent")
    }
}
