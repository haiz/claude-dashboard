import XCTest
@testable import ClaudeDashboard

@MainActor
final class CriticalSettingsTests: XCTestCase {
    private var suite: String!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suite)
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        super.tearDown()
    }

    func testDefaults() {
        let s = CriticalSettings(defaults: defaults)
        XCTAssertEqual(s.pace, 50)
        XCTAssertEqual(s.highPace, 80)
        XCTAssertEqual(s.quietStart, 0)
        XCTAssertEqual(s.quietEnd, 7)
        XCTAssertEqual(s.timeZoneId, "Asia/Ho_Chi_Minh")
        XCTAssertEqual(s.quietHours, QuietHours(startHour: 0, endHour: 7, timeZone: TimeZone(identifier: "Asia/Ho_Chi_Minh")!))
    }

    func testValuesPersistAcrossInstances() {
        let s = CriticalSettings(defaults: defaults)
        s.pace = 40
        s.highPace = 90
        s.quietStart = 23
        s.quietEnd = 6
        s.timeZoneId = "Europe/Paris"
        let t = CriticalSettings(defaults: defaults)
        XCTAssertEqual([t.pace, t.highPace, t.quietStart, t.quietEnd], [40, 90, 23, 6])
        XCTAssertEqual(t.timeZoneId, "Europe/Paris")
    }

    func testRaisingPaceAboveHighPaceRaisesHighPace() {
        let s = CriticalSettings(defaults: defaults)
        s.pace = 85
        XCTAssertEqual(s.highPace, 85)
        XCTAssertEqual(CriticalSettings(defaults: defaults).highPace, 85)
    }

    func testHighPaceCannotGoBelowPace() {
        let s = CriticalSettings(defaults: defaults)
        s.highPace = 30
        XCTAssertEqual(s.highPace, 50)
    }

    func testInvalidStoredValuesFallBack() {
        defaults.set(5, forKey: CriticalSettings.paceKey)
        defaults.set(40, forKey: CriticalSettings.highPaceKey)
        defaults.set(30, forKey: CriticalSettings.quietStartKey)
        defaults.set(-1, forKey: CriticalSettings.quietEndKey)
        defaults.set("Mars/Olympus", forKey: CriticalSettings.timeZoneKey)
        let s = CriticalSettings(defaults: defaults)
        XCTAssertEqual(s.pace, 50)
        XCTAssertEqual(s.highPace, 50)   // stored 40 < pace: lifted to pace
        XCTAssertEqual(s.quietStart, 0)
        XCTAssertEqual(s.quietEnd, 7)
        XCTAssertEqual(s.timeZoneId, "Asia/Ho_Chi_Minh")
    }

    func testEqualStartAndEndMeansNoQuietHours() {
        let s = CriticalSettings(defaults: defaults)
        s.quietEnd = 0
        XCTAssertNil(s.quietHours)
    }

    func testParameters() {
        let p = CriticalSettings(defaults: defaults).parameters(x: 16.3)
        XCTAssertEqual(p, CriticalParameters(x: 16.3, pace: 50, highPace: 80,
                                             quiet: QuietHours(startHour: 0, endHour: 7, timeZone: TimeZone(identifier: "Asia/Ho_Chi_Minh")!)))
    }

    /// From pace 90 / high 95, resetting pace first must not leave high pace at 95.
    func testResetToDefaults() {
        let s = CriticalSettings(defaults: defaults)
        s.pace = 90
        s.highPace = 95
        s.quietStart = 22
        s.quietEnd = 5
        s.timeZoneId = "Europe/Paris"

        s.resetToDefaults()

        for t in [s, CriticalSettings(defaults: defaults)] {
            XCTAssertEqual([t.pace, t.highPace, t.quietStart, t.quietEnd], [50, 80, 0, 7])
            XCTAssertEqual(t.timeZoneId, "Asia/Ho_Chi_Minh")
        }
    }
}
