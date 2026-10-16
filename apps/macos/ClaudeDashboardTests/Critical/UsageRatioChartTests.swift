import XCTest
@testable import ClaudeDashboard

@MainActor
final class UsageRatioChartTests: XCTestCase {
    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    func testLabelMeasured() {
        XCTAssertEqual(UsageRatioButton.label(UsageRatioValue(x: 16.27, updatedAt: t0)), "X 16.3%")
    }

    func testLabelDefault() {
        XCTAssertEqual(UsageRatioButton.label(UsageRatioValue(x: 16, updatedAt: nil)), "X 16% (default)")
    }

    func testHelpNamesTheValueAndTheUpdateTime() {
        let help = UsageRatioButton.help(UsageRatioValue(x: 16.27, updatedAt: t0))
        XCTAssertTrue(help.hasPrefix("100% of a 5h window costs about 16.3% of the 7d quota."), help)
        XCTAssertTrue(help.contains("updated hourly"), help)
    }

    func testHelpForTheDefaultSaysSo() {
        XCTAssertTrue(UsageRatioButton.help(UsageRatioValue(x: 16, updatedAt: nil)).contains("not enough usage history yet"))
    }

    func testNearestPoint() {
        let points = [0.0, 3600, 7200].map { UsageRatioPoint(date: t0.addingTimeInterval($0), x: $0, sumFiveHour: 0, sumSevenDay: 0) }
        XCTAssertEqual(UsageRatioChart.nearest(points, to: t0.addingTimeInterval(4000))?.x, 3600)
        XCTAssertNil(UsageRatioChart.nearest([], to: t0))
    }

    func testViewModelOwnsARatioController() async throws {
        let suite = StoreFixture.makeSuiteName()
        defer { StoreFixture.destroy(suite: suite) }
        let logPath = NSTemporaryDirectory() + "usage_\(UUID().uuidString).db"
        defer { try? FileManager.default.removeItem(atPath: logPath) }
        let vm = DashboardViewModel(accountStore: AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite))),
                                    logStore: UsageLogStore(dbPath: logPath))
        await vm.ratioController.recompute()   // empty logs: stays at the default
        XCTAssertEqual(vm.ratioController.current, UsageRatioValue(x: 16, updatedAt: nil))
    }
}
