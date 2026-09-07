import XCTest
@testable import ClaudeDashboard

final class AccountStoreTests: XCTestCase {

    private var store: AccountStore!
    private var defaults: UserDefaults!
    private var suiteName: String!

    override func setUp() {
        super.setUp()
        // A UUID suite per test, not one fixed name: that is what makes the
        // old `removePersistentDomain` in setUp unnecessary, and what lets
        // `StoreFixture.destroy` unlink the plist instead of leaving it in
        // ~/Library/Preferences after the run.
        suiteName = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suiteName)!
        store = AccountStore(defaults: defaults)
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suiteName)
        super.tearDown()
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
            status: .active,
            source: .browser
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
            status: .active,
            source: .browser
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
            status: .active,
            source: .browser
        )

        store.addAccount(account)
        account.name = "New Name"
        store.updateAccount(account)

        XCTAssertEqual(store.accounts.first?.name, "New Name")
    }

    /// `contract/account-schema.md` "An unreadable store is not an empty
    /// store". Every mutation here calls `persist()`, so bytes read as an
    /// empty list are gone on the user's very next add.
    func testUnparseableStoreIsKeptAsideRatherThanOverwritten() {
        let garbage = Data(#"[{"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"tru"#.utf8)
        defaults.set(garbage, forKey: StoreFixture.storageKey)

        let recovered = AccountStore(defaults: defaults)
        recovered.addAccount(
            Account(
                id: UUID(),
                name: "Fresh",
                email: nil,
                chromeProfilePath: "Profile 1",
                chromeProfileName: nil,
                orgId: "org-123",
                plan: .pro,
                lastSynced: nil,
                status: .active,
                source: .browser
            )
        )

        let kept = StoreFixture.unreadableCopies(inSuite: suiteName)
        XCTAssertEqual(kept.count, 1, "expected one kept copy, found \(kept.keys)")
        XCTAssertEqual(kept.values.first, garbage, "byte for byte")
        XCTAssertEqual(recovered.accounts.count, 1, "and the store still works")
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
            status: .active,
            source: .browser
        )

        store.addAccount(account)

        let store2 = AccountStore(defaults: defaults)
        XCTAssertEqual(store2.accounts.count, 1)
        XCTAssertEqual(store2.accounts.first?.name, "Persistent")
    }
}
