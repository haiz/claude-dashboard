import XCTest
@testable import ClaudeDashboard

/// Drives `AccountIdentity.resolveOrgId` from `contract/cases/org-selection.json`.
final class OrgSelectionContractTests: XCTestCase {

    func testOrgSelectionCases() throws {
        let data = try ContractFixtures.data("cases/org-selection.json")
        guard let cases = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return XCTFail("cases/org-selection.json must be an array of objects")
        }

        XCTAssertFalse(cases.isEmpty, "contract/cases/org-selection.json is empty")

        for c in cases {
            let name = c["name"] as? String ?? "<unnamed>"
            let lastActiveOrg = c["last_active_org"] as? String   // absent or null → nil
            let raw = c["memberships"] as? [[String: Any]] ?? []
            let memberships = raw.map {
                OrgMembership(
                    uuid: $0["uuid"] as? String ?? "",
                    name: $0["name"] as? String ?? "",
                    capabilities: $0["capabilities"] as? [String] ?? []
                )
            }
            let expected = c["expect_org_id"] as? String          // absent or null → nil

            let actual = AccountIdentity.resolveOrgId(
                lastActiveOrg: lastActiveOrg, memberships: memberships)

            XCTAssertEqual(actual, expected, "case: \(name)")
        }
    }
}
