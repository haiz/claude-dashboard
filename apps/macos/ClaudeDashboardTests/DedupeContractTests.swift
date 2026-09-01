import XCTest
@testable import ClaudeDashboard

/// Drives `AccountIdentity.isDuplicate` from `contract/cases/dedupe.json`.
final class DedupeContractTests: XCTestCase {

    func testDedupeCases() throws {
        let data = try ContractFixtures.data("cases/dedupe.json")
        guard let cases = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return XCTFail("cases/dedupe.json must be an array of objects")
        }

        XCTAssertFalse(cases.isEmpty, "contract/cases/dedupe.json is empty")

        for c in cases {
            let name = c["name"] as? String ?? "<unnamed>"
            guard let candidate = c["candidate"] as? [String: Any],
                  let candidateUuid = candidate["account_uuid"] as? String else {
                XCTFail("case '\(name)' needs candidate.account_uuid")
                continue
            }
            let candidateEmail = candidate["email"] as? String
            let stored = (c["stored"] as? [[String: Any]] ?? []).map {
                StoredIdentity(
                    accountUuid: $0["account_uuid"] as? String,
                    email: $0["email"] as? String
                )
            }
            let expected = c["expect_duplicate"] as? Bool ?? false

            let actual = AccountIdentity.isDuplicate(
                candidateUuid: candidateUuid, candidateEmail: candidateEmail, against: stored)

            XCTAssertEqual(actual, expected, "case: \(name)")
        }
    }
}
