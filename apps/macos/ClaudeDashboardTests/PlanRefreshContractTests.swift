import XCTest
@testable import ClaudeDashboard

/// Drives `UsageAPIService.refreshedPlan` from `contract/cases/plan-refresh.json` —
/// the same file `apps/linux/core/tests/contract_plan_refresh.rs` reads.
final class PlanRefreshContractTests: XCTestCase {

    func testPlanRefreshCases() throws {
        let data = try ContractFixtures.data("cases/plan-refresh.json")
        guard let cases = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return XCTFail("cases/plan-refresh.json must be an array of objects")
        }

        XCTAssertFalse(cases.isEmpty, "contract/cases/plan-refresh.json is empty")

        for c in cases {
            let name = c["name"] as? String ?? "<unnamed>"
            guard let storedWire = c["stored_plan"] as? String,
                  let stored = AccountPlan(rawValue: storedWire) else {
                XCTFail("case '\(name)' has no valid `stored_plan`")
                continue
            }
            // absent or null → nil (an unresolvable hint)
            let hint = (c["hint_plan"] as? String).flatMap(AccountPlan.init(rawValue:))
            let expected = c["expect_write"] as? String

            let actual = UsageAPIService.refreshedPlan(stored: stored, hint: hint)

            XCTAssertEqual(actual?.rawValue, expected, "case: \(name)")
        }
    }

    // MARK: - Matching the org the account is polled against

    private func makeAccount(orgId: String?, plan: AccountPlan) -> Account {
        Account(
            id: UUID(),
            name: "Test",
            email: nil,
            chromeProfilePath: "Profile 1",
            chromeProfileName: nil,
            orgId: orgId,
            accountUuid: "acct-1",
            plan: plan,
            lastSynced: nil,
            status: .active,
            source: .browser
        )
    }

    private func makeOrg(uuid: String, plan: AccountPlan?) -> OrgInfo {
        OrgInfo(uuid: uuid, name: "Personal", email: nil, capabilities: [], planHint: plan)
    }

    func testRefreshedPlanTakesTheHintOfTheStoredOrg() {
        let account = makeAccount(orgId: "org-1", plan: .pro)
        let orgs = [makeOrg(uuid: "org-2", plan: .max5x), makeOrg(uuid: "org-1", plan: .max200)]

        XCTAssertEqual(UsageAPIService.refreshedPlan(for: account, orgs: orgs), .max200)
    }

    func testRefreshedPlanIsNilWhenTheFetchFailed() {
        let account = makeAccount(orgId: "org-1", plan: .pro)

        XCTAssertNil(UsageAPIService.refreshedPlan(for: account, orgs: nil),
                     "a failed fetch must never re-fabricate a tier")
    }

    func testRefreshedPlanIsNilWhenNoOrgMatchesTheStoredOrgId() {
        let account = makeAccount(orgId: "org-1", plan: .pro)
        let orgs = [makeOrg(uuid: "org-2", plan: .max200)]

        XCTAssertNil(UsageAPIService.refreshedPlan(for: account, orgs: orgs),
                     "another org's tier must not leak into this account")
    }

    func testRefreshedPlanIsNilForAnAccountWithNoOrgId() {
        let account = makeAccount(orgId: nil, plan: .pro)
        let orgs = [makeOrg(uuid: "org-1", plan: .max200)]

        XCTAssertNil(UsageAPIService.refreshedPlan(for: account, orgs: orgs))
    }
}
