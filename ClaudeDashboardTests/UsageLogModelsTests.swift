// ClaudeDashboardTests/UsageLogModelsTests.swift
import XCTest
@testable import ClaudeDashboard

final class UsageLogModelsTests: XCTestCase {
    func testBurnRateLevel1_over5hours() {
        let result = BurnRateResult.fromProjectedTime(6 * 3600) // 6 hours
        XCTAssertEqual(result.level, 1)
        XCTAssertEqual(result.animal, "🐌")
    }

    func testBurnRateLevel2_3to5hours() {
        let result = BurnRateResult.fromProjectedTime(4 * 3600) // 4 hours
        XCTAssertEqual(result.level, 2)
        XCTAssertEqual(result.animal, "🐢")
    }

    func testBurnRateLevel3_1point5to3hours() {
        let result = BurnRateResult.fromProjectedTime(2 * 3600) // 2 hours
        XCTAssertEqual(result.level, 3)
        XCTAssertEqual(result.animal, "🐇")
    }

    func testBurnRateLevel4_30mto1point5hours() {
        let result = BurnRateResult.fromProjectedTime(1 * 3600) // 1 hour
        XCTAssertEqual(result.level, 4)
        XCTAssertEqual(result.animal, "🐎")
    }

    func testBurnRateLevel5_under30min() {
        let result = BurnRateResult.fromProjectedTime(15 * 60) // 15 min
        XCTAssertEqual(result.level, 5)
        XCTAssertEqual(result.animal, "🐆")
    }

    func testBurnRateBoundary_exactly5hours() {
        let result = BurnRateResult.fromProjectedTime(5 * 3600)
        XCTAssertEqual(result.level, 2) // 5h is not > 5h, so level 2
    }

    func testBurnRateBoundary_exactly30min() {
        let result = BurnRateResult.fromProjectedTime(30 * 60)
        XCTAssertEqual(result.level, 5) // 0.5h is not > 0.5h, so level 5
    }

    func testUsageWindowLabels() {
        XCTAssertEqual(UsageWindow.fiveHour.label, "5h")
        XCTAssertEqual(UsageWindow.sevenDay.label, "7d")
        XCTAssertEqual(UsageWindow.sonnet.label, "S")
    }

    func testUsageWindowRawValues() {
        XCTAssertEqual(UsageWindow.fiveHour.rawValue, 0)
        XCTAssertEqual(UsageWindow.sevenDay.rawValue, 1)
        XCTAssertEqual(UsageWindow.sonnet.rawValue, 2)
    }
}
