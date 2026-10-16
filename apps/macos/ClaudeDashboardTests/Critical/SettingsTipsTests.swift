import XCTest
@testable import ClaudeDashboard

final class SettingsTipsTests: XCTestCase {
    func testEveryTipHasAllParts() {
        XCTAssertEqual(SettingsTips.all.count, 12)
        for tip in SettingsTips.all {
            XCTAssertFalse(tip.summary.isEmpty)
            XCTAssertFalse(tip.detail.isEmpty, tip.summary)
            XCTAssertFalse(tip.example.isEmpty, tip.summary)
        }
    }

    func testCriticalTipsLinkToTheCriticalTopic() {
        for tip in [SettingsTips.criticalPace, SettingsTips.highCriticalPace, SettingsTips.quietHours, SettingsTips.timeZone] {
            XCTAssertEqual(tip.topic, .criticalAccounts, tip.summary)
        }
    }

    /// The pace example's arithmetic must hold: X 16, pace 50 -> 8 per window.
    func testCriticalPaceExampleNumbers() {
        XCTAssertTrue(SettingsTips.criticalPace.example.contains("60 + 48 = 108"))
        XCTAssertTrue(SettingsTips.criticalPace.example.contains("55 + 40 = 95"))
        XCTAssertEqual(CriticalPlanner.projected(sevenDay: 60, activeHours: 30, pace: 50, x: 16), 108, accuracy: 1e-9)
        XCTAssertEqual(CriticalPlanner.projected(sevenDay: 55, activeHours: 25, pace: 50, x: 16), 95, accuracy: 1e-9)
    }

    func testHelpHasTheCriticalTopic() {
        XCTAssertTrue(HelpTopic.allCases.contains(.criticalAccounts))
        XCTAssertEqual(HelpTopic.criticalAccounts.title, "Critical Accounts")
    }
}
