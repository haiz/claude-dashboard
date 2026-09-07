import XCTest

/// The store seam: without it, a real-binary test writes into the user's own
/// account store. cfprefsd does not follow HOME, so overriding the suite name
/// is the macOS equivalent of Linux's XDG_CONFIG_HOME.
final class HelperAccountStoreTests: XCTestCase {

    private var suite: String!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        setenv(HelperAccountStore.suiteVariable, suite, 1)
    }

    override func tearDown() {
        unsetenv(HelperAccountStore.suiteVariable)
        StoreFixture.destroy(suite: suite)
        super.tearDown()
    }

    func testDefaultSuiteIsTheProductionOne() {
        unsetenv(HelperAccountStore.suiteVariable)
        XCTAssertEqual(HelperAccountStore.resolvedSuiteName(), "com.claude-dashboard.app")
    }

    func testOverrideIsHonoured() {
        XCTAssertEqual(HelperAccountStore.resolvedSuiteName(), suite)
    }

    func testRoundTripsThroughTheOverriddenSuite() {
        let account = StoreFixture.account(name: "person@example.com", orgId: "org-1")
        HelperAccountStore.saveAccounts([account])

        XCTAssertEqual(HelperAccountStore.loadAccounts(), [account])
        // And the bytes really landed in the temp suite, not the real one.
        XCTAssertEqual(StoreFixture.read(fromSuite: suite), [account])
    }

    func testEmptyStoreLoadsAsEmptyArray() {
        XCTAssertEqual(HelperAccountStore.loadAccounts(), [])
    }

    /// `contract/account-schema.md` "An unreadable store is not an empty
    /// store": for a writer, bytes that will not decode are moved aside and
    /// named, leaving the store absent rather than corrupt.
    func testUnparseableStoreIsMovedAsideForAWriter() throws {
        let garbage = Data(#"[{"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"tru"#.utf8)
        StoreFixture.seedData(garbage, intoSuite: suite)

        let loaded = HelperAccountStore.loadAccountsForWrite()

        XCTAssertEqual(loaded.accounts, [])
        let kept = try XCTUnwrap(loaded.quarantined, "the bytes must be kept somewhere")
        XCTAssertEqual(StoreFixture.unreadableCopies(inSuite: suite), [kept: garbage])
        XCTAssertNil(
            StoreFixture.readData(fromSuite: suite),
            "the store is absent now, not corrupt"
        )
    }

    /// The read-only load is deliberately left alone: moving the bytes aside
    /// is a write, and `decrypt` has nothing to protect by doing it.
    func testTheReadOnlyLoadStillReportsAnUnparseableStoreAsEmpty() {
        let garbage = Data(#"[{"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"tru"#.utf8)
        StoreFixture.seedData(garbage, intoSuite: suite)

        XCTAssertEqual(HelperAccountStore.loadAccounts(), [])
        XCTAssertTrue(StoreFixture.unreadableCopies(inSuite: suite).isEmpty)
        XCTAssertNotNil(StoreFixture.readData(fromSuite: suite), "and nothing was moved")
    }

    func testDestroyUnlinksTheBackingPlistFile() {
        let plistPath = NSHomeDirectory() + "/Library/Preferences/\(suite!).plist"

        let account = StoreFixture.account(name: "person@example.com", orgId: "org-1")
        StoreFixture.seed([account], intoSuite: suite)
        UserDefaults(suiteName: suite)?.synchronize()
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: plistPath),
            "seeding should have written a plist to disk"
        )

        StoreFixture.destroy(suite: suite)

        XCTAssertFalse(
            FileManager.default.fileExists(atPath: plistPath),
            "destroy should remove the backing plist file, not just clear its data"
        )
    }
}
