import XCTest
@testable import ClaudeDashboard

/// Drives `UsageAPIService.detectPlanTier` from `contract/cases/plan-detection.json`.
final class PlanDetectionContractTests: XCTestCase {

    func testPlanDetectionCases() throws {
        let data = try ContractFixtures.data("cases/plan-detection.json")
        guard let cases = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return XCTFail("cases/plan-detection.json must be an array of objects")
        }

        XCTAssertFalse(cases.isEmpty, "contract/cases/plan-detection.json is empty")

        for c in cases {
            let name = c["name"] as? String ?? "<unnamed>"
            guard let org = c["org"] as? [String: Any] else {
                XCTFail("case '\(name)' has no `org` object")
                continue
            }
            let capabilities = org["capabilities"] as? [String] ?? []
            let expected = c["expect_plan"] as? String   // absent or null → nil

            let actual = UsageAPIService.detectPlanTier(from: org, capabilities: capabilities)

            XCTAssertEqual(actual?.rawValue, expected, "case: \(name)")
        }
    }
}
