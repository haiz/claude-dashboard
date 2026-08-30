import XCTest
@testable import ClaudeDashboard

/// Drives `BurnRateResult.fromProjectedTime` from
/// `contract/cases/burn-rate-levels.json`.
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
}
