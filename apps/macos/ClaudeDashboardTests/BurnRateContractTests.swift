import XCTest
@testable import ClaudeDashboard

/// Drives `BurnRateResult` from `contract/cases/burn-rate-levels.json`, `burn-rate-rocket.json`,
/// `burn-rate-tracker.json` and `fast-refresh.json`.
final class BurnRateContractTests: XCTestCase {

    private struct Case: Decodable {
        let name: String
        let projectedSeconds: Double
        let expectLevel: Int
        let expectAnimal: String

        enum CodingKeys: String, CodingKey {
            case name
            case projectedSeconds = "projected_seconds"
            case expectLevel = "expect_level"
            case expectAnimal = "expect_animal"
        }
    }

    func testBurnRateLevelCases() throws {
        let data = try ContractFixtures.data("cases/burn-rate-levels.json")
        let cases = try JSONDecoder().decode([Case].self, from: data)

        XCTAssertFalse(cases.isEmpty, "contract/cases/burn-rate-levels.json is empty")

        for c in cases {
            let result = BurnRateResult.fromProjectedTime(c.projectedSeconds)
            XCTAssertEqual(result.level, c.expectLevel, "level — case: \(c.name)")
            XCTAssertEqual(result.animal, c.expectAnimal, "animal — case: \(c.name)")
            XCTAssertEqual(result.projectedTime, c.projectedSeconds,
                           "projectedTime is passed through — case: \(c.name)")
        }
    }

    private struct RocketCase: Decodable {
        let name: String
        let ratePctPerMin: Double
        let sustainedSeconds: Double
        let utilization: Double
        let expectLevel: Int
        let expectAnimal: String
        let expectProjectedSeconds: Double

        enum CodingKeys: String, CodingKey {
            case name, utilization
            case ratePctPerMin = "rate_pct_per_min"
            case sustainedSeconds = "sustained_seconds"
            case expectLevel = "expect_level"
            case expectAnimal = "expect_animal"
            case expectProjectedSeconds = "expect_projected_seconds"
        }
    }

    func testRocketCases() throws {
        let cases = try JSONDecoder().decode(
            [RocketCase].self, from: ContractFixtures.data("cases/burn-rate-rocket.json"))
        XCTAssertFalse(cases.isEmpty)
        for c in cases {
            let r = BurnRateResult.from(ratePerMinute: c.ratePctPerMin,
                                        sustainedSeconds: c.sustainedSeconds, utilization: c.utilization)
            XCTAssertEqual(r.level, c.expectLevel, "level - case: \(c.name)")
            XCTAssertEqual(r.animal, c.expectAnimal, "animal - case: \(c.name)")
            XCTAssertEqual(r.projectedTime, c.expectProjectedSeconds, accuracy: 1e-9,
                           "projected - case: \(c.name)")
        }
        XCTAssertEqual(BurnRateResult.rocketLevel, 6)
    }

    private struct FastCase: Decodable {
        let name: String
        let fiveHourLevel: Int?
        let isActive: Bool
        let hasError: Bool
        let autoRefreshEnabled: Bool
        let expect: Bool

        enum CodingKeys: String, CodingKey {
            case name, expect
            case fiveHourLevel = "five_hour_level"
            case isActive = "is_active"
            case hasError = "has_error"
            case autoRefreshEnabled = "auto_refresh_enabled"
        }
    }

    func testFastRefreshCases() throws {
        let cases = try JSONDecoder().decode(
            [FastCase].self, from: ContractFixtures.data("cases/fast-refresh.json"))
        XCTAssertFalse(cases.isEmpty)
        for c in cases {
            XCTAssertEqual(
                BurnRateResult.needsFastRefresh(fiveHourLevel: c.fiveHourLevel, isActive: c.isActive,
                                                hasError: c.hasError, autoRefreshEnabled: c.autoRefreshEnabled),
                c.expect, "case: \(c.name)")
        }
    }
}
