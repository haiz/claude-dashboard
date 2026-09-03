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

            // `sync`'s plan heal needs *which* stored account matched, so the
            // two functions must never disagree about whether one did.
            let index = AccountIdentity.duplicateIndex(
                candidateUuid: candidateUuid, candidateEmail: candidateEmail, against: stored)

            XCTAssertEqual(index != nil, expected, "case: \(name) (duplicateIndex)")
        }
    }

    func testDuplicateIndexPointsAtTheMatchingEntry() {
        let stored = [
            StoredIdentity(accountUuid: "acct-1", email: "one@example.com"),
            StoredIdentity(accountUuid: "acct-2", email: "two@example.com")
        ]

        XCTAssertEqual(
            AccountIdentity.duplicateIndex(
                candidateUuid: "acct-2", candidateEmail: "two@example.com", against: stored),
            1)
    }

    func testDuplicateIndexFindsTheLegacyEmailMatch() {
        let stored = [
            StoredIdentity(accountUuid: "acct-1", email: "one@example.com"),
            StoredIdentity(accountUuid: nil, email: "Legacy@Example.com")
        ]

        XCTAssertEqual(
            AccountIdentity.duplicateIndex(
                candidateUuid: "acct-9", candidateEmail: "legacy@example.com", against: stored),
            1)
    }

    func testDuplicateIndexIsNilForANewAccount() {
        let stored = [StoredIdentity(accountUuid: "acct-1", email: "one@example.com")]

        XCTAssertNil(AccountIdentity.duplicateIndex(
            candidateUuid: "acct-2", candidateEmail: "two@example.com", against: stored))
    }
}
