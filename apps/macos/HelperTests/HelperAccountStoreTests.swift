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
