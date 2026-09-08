import XCTest
@testable import ClaudeDashboard

final class AppDefaultsTests: XCTestCase {

    /// A normal launch writes where the user's accounts actually live.
    func testNormalLaunchUsesTheStandardDomain() {
        XCTAssertNil(AppDefaults.suiteName(override: nil, isRunningTests: false))
    }

    /// The app target is the test host, so `xcodebuild test` launches the real
    /// app, which builds its stores against `UserDefaults.standard` — the
    /// developer's own accounts. Anything those stores write at startup then
    /// lands on a real machine. A suite of its own is what keeps a test run
    /// from touching them.
    func testATestRunIsDivertedToASuiteOfItsOwn() {
        let suite = AppDefaults.suiteName(override: nil, isRunningTests: true)

        XCTAssertNotNil(suite)
        XCTAssertNotEqual(suite, "com.claude-dashboard.app")
    }

    /// The same variable the helper CLI reads, so a test that drives both
    /// binaries can point them at one suite (`HelperAccountStore.suiteVariable`).
    func testAnExplicitSuiteWinsOverEverything() {
        XCTAssertEqual(
            AppDefaults.suiteName(override: "custom.suite", isRunningTests: true),
            "custom.suite")
        XCTAssertEqual(
            AppDefaults.suiteName(override: "custom.suite", isRunningTests: false),
            "custom.suite")
    }

    /// An empty variable is an unset one, not a suite named "".
    func testAnEmptyOverrideIsIgnored() {
        XCTAssertNil(AppDefaults.suiteName(override: "", isRunningTests: false))
    }

    /// The test bundle itself runs inside the host, so this asserts on the
    /// live value: if the app ever went back to the standard domain under
    /// test, this fails.
    func testTheRunningTestHostIsNotOnTheUsersOwnDomain() {
        XCTAssertFalse(AppDefaults.shared === UserDefaults.standard)
    }
}
