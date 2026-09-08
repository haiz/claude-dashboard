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
    // MARK: - Saved run commands

    /// A saved command is keyed by `Account.id`, so a deleted account's
    /// command can never be reached again. Leaving it behind means the key
    /// outlives every account that could read it.
    func testRemovingAnAccountDropsItsSavedCommand() {
        let account = Account(
            id: UUID(),
            name: "Test Account",
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
        defaults.set("claude -p ping", forKey: RunCommandSettings.commandKey(for: account.id))
        defaults.set(true, forKey: RunCommandSettings.terminalKey(for: account.id))

        store.removeAccount(id: account.id)

        XCTAssertNil(defaults.string(forKey: RunCommandSettings.commandKey(for: account.id)))
        XCTAssertNil(defaults.object(forKey: RunCommandSettings.terminalKey(for: account.id)))
    }

    /// Keys left behind by earlier versions, or by a delete that predates the
    /// cleanup above. Loading the store is the one moment that knows the full
    /// set of live ids, so it is where they go — and a live account's own
    /// command must survive it.
    func testLoadingTheStorePrunesCommandsOfAccountsThatAreGone() {
        let live = Account(
            id: UUID(),
            name: "Live",
            email: nil,
            chromeProfilePath: "Profile 1",
            chromeProfileName: nil,
            orgId: "org-123",
            plan: .pro,
            lastSynced: nil,
            status: .active,
            source: .browser
        )
        store.addAccount(live)
        defaults.set("claude -p live", forKey: RunCommandSettings.commandKey(for: live.id))

        let orphan = UUID()
        defaults.set("claude -p orphan", forKey: RunCommandSettings.commandKey(for: orphan))
        defaults.set(true, forKey: RunCommandSettings.terminalKey(for: orphan))

        // A second store over the same defaults: what the next launch does.
        _ = AccountStore(defaults: defaults)

        XCTAssertNil(defaults.string(forKey: RunCommandSettings.commandKey(for: orphan)))
        XCTAssertNil(defaults.object(forKey: RunCommandSettings.terminalKey(for: orphan)))
        XCTAssertEqual(
            defaults.string(forKey: RunCommandSettings.commandKey(for: live.id)),
            "claude -p live")
    }

    /// The store's own rule is that unreadable bytes are not an empty store
    /// (`contract/account-schema.md`). The pruning above reads the decoded
    /// list to decide what is dead, so on a decode failure it would see zero
    /// live accounts and delete every saved command the user has — destroying
    /// exactly the data the quarantine exists to protect.
    func testUnreadableStoreDoesNotPruneAnyCommand() {
        let survivor = UUID()
        defaults.set("claude -p keep-me", forKey: RunCommandSettings.commandKey(for: survivor))
        defaults.set(Data("not json".utf8), forKey: "claude-dashboard.accounts")

        _ = AccountStore(defaults: defaults)

        XCTAssertEqual(
            defaults.string(forKey: RunCommandSettings.commandKey(for: survivor)),
            "claude -p keep-me")
    }

}
