import XCTest
@testable import ClaudeDashboard

/// Drives `ManualKey.decision` from `contract/cases/manual-key.json` — the same
/// file `apps/linux/core/tests/contract_manual_key.rs` reads.
final class ManualKeyContractTests: XCTestCase {

    func testManualKeyCases() throws {
        let data = try ContractFixtures.data("cases/manual-key.json")
        guard let cases = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return XCTFail("cases/manual-key.json must be an array of objects")
        }
        XCTAssertFalse(cases.isEmpty, "contract/cases/manual-key.json is empty")

        for c in cases {
            let name = c["name"] as? String ?? "<unnamed>"

            let stored = (c["stored"] as? [String: Any]).map {
                StoredManualTarget(
                    orgId: $0["org_id"] as? String,
                    accountUuid: $0["account_uuid"] as? String,
                    email: $0["email"] as? String)
            }
            guard let fetched = c["fetched"] as? [String: Any],
                  let fetchedUuid = fetched["uuid"] as? String else {
                XCTFail("case '\(name)' needs fetched.uuid")
                continue
            }
            let memberships = (c["memberships"] as? [[String: Any]] ?? []).map {
                OrgMembership(
                    uuid: $0["uuid"] as? String ?? "",
                    name: $0["name"] as? String ?? "",
                    capabilities: $0["capabilities"] as? [String] ?? [])
            }

            let actual = ManualKey.decision(
                stored: stored,
                fetchedUuid: fetchedUuid,
                fetchedEmail: fetched["email"] as? String,
                memberships: memberships)

            guard let expect = c["expect"] as? [String: Any],
                  let action = expect["action"] as? String else {
                XCTFail("case '\(name)' needs expect.action")
                continue
            }

            switch action {
            case "add":
                XCTAssertEqual(actual, .add(orgId: expect["org_id"] as? String ?? ""),
                               "case: \(name)")
            case "reject_no_chat_org":
                XCTAssertEqual(actual, .rejectNoChatOrg, "case: \(name)")
            case "repair":
                let w = expect["writes"] as? [String: Any] ?? [:]
                let warn = try XCTUnwrap(expect["warn_no_chat_org"] as? Bool,
                                         "case '\(name)' — a repair needs expect.warn_no_chat_org")
                XCTAssertEqual(actual, .repair(writes: ManualKeyWrites(
                    orgId: w["org_id"] as? String,
                    accountUuid: w["account_uuid"] as? String,
                    email: w["email"] as? String), warnNoChatOrg: warn),
                    "case: \(name)")
            default:
                XCTFail("case '\(name)' — unknown action \(action)")
            }
        }
    }
}
