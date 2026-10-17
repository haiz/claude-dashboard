import XCTest
@testable import ClaudeDashboard

@MainActor
final class AutoSwitchSettingsTests: XCTestCase {

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
        let s = AutoSwitchSettings(defaults: defaults)
        XCTAssertTrue(s.isEnabled)
        XCTAssertEqual(s.fiveHourThreshold, 95)
        XCTAssertEqual(s.sevenDayThreshold, 99)
        XCTAssertTrue(s.showWidgetToggle)
        XCTAssertNil(s.priority)
        XCTAssertEqual(s.thresholds, AutoSwitchThresholds(fiveHour: 95, sevenDay: 99))
    }

    /// On by default only when nothing is stored: a user who turned it off stays off.
    func testStoredOffSurvivesTheOnDefault() {
        defaults.set(false, forKey: AutoSwitchSettings.enabledKey)
        XCTAssertFalse(AutoSwitchSettings(defaults: defaults).isEnabled)
    }

    func testValuesPersistAcrossInstances() {
        let s = AutoSwitchSettings(defaults: defaults)
        s.isEnabled = true
        s.fiveHourThreshold = 90
        s.sevenDayThreshold = 97
        s.showWidgetToggle = false
        let id = UUID()
        let baseline = Date(timeIntervalSince1970: 1_800_000_000)
        s.setPriority(AutoSwitchPriority(accountId: id, baseline: baseline))

        let reloaded = AutoSwitchSettings(defaults: defaults)
        XCTAssertTrue(reloaded.isEnabled)
        XCTAssertEqual(reloaded.fiveHourThreshold, 90)
        XCTAssertEqual(reloaded.sevenDayThreshold, 97)
        XCTAssertFalse(reloaded.showWidgetToggle)
        XCTAssertEqual(reloaded.priority, AutoSwitchPriority(accountId: id, baseline: baseline))
    }

    func testPriorityWithNilBaselineAndClearing() {
        let s = AutoSwitchSettings(defaults: defaults)
        let id = UUID()
        s.setPriority(AutoSwitchPriority(accountId: id, baseline: Date()))
        s.setPriority(AutoSwitchPriority(accountId: id, baseline: nil))
        XCTAssertEqual(AutoSwitchSettings(defaults: defaults).priority, AutoSwitchPriority(accountId: id, baseline: nil))
        s.setPriority(nil)
        XCTAssertNil(AutoSwitchSettings(defaults: defaults).priority)
    }

    func testOutOfRangeStoredThresholdsFallBackToDefaults() {
        defaults.set(10, forKey: AutoSwitchSettings.fiveHourKey)
        defaults.set(101, forKey: AutoSwitchSettings.sevenDayKey)
        let s = AutoSwitchSettings(defaults: defaults)
        XCTAssertEqual(s.fiveHourThreshold, 95)
        XCTAssertEqual(s.sevenDayThreshold, 99)
    }

    /// Reset restores every setting but keeps the home account, which is not a setting.
    func testResetToDefaultsKeepsPriority() {
        let s = AutoSwitchSettings(defaults: defaults)
        s.isEnabled = false
        s.fiveHourThreshold = 60
        s.sevenDayThreshold = 70
        s.showWidgetToggle = false
        let priority = AutoSwitchPriority(accountId: UUID(), baseline: nil)
        s.setPriority(priority)

        s.resetToDefaults()

        let reloaded = AutoSwitchSettings(defaults: defaults)
        for settings in [s, reloaded] {
            XCTAssertTrue(settings.isEnabled)
            XCTAssertEqual(settings.thresholds, AutoSwitchThresholds(fiveHour: 95, sevenDay: 99))
            XCTAssertTrue(settings.showWidgetToggle)
            XCTAssertEqual(settings.priority, priority)
        }
    }
}
