import XCTest
@testable import ClaudeDashboard

/// `source` is a schema field two implementations must agree on. These guard the
/// half the contract case file cannot: that the field survives a round trip and
/// that a record written before it existed still decodes.
final class AccountSourceTests: XCTestCase {

    private func account(source: AccountSource) -> Account {
        Account(
            id: UUID(),
            name: "person@example.com",
            email: "person@example.com",
            chromeProfilePath: "",
            chromeProfileName: nil,
            orgId: "org-good",
            accountUuid: "acct-1",
            sessionKey: nil,
            browser: .chrome,
            plan: .pro,
            lastSynced: nil,
            status: .active,
            source: source
        )
    }

    func testManualSourceSurvivesARoundTrip() throws {
        let data = try JSONEncoder().encode(account(source: .manual))
        let decoded = try JSONDecoder().decode(Account.self, from: data)

        XCTAssertEqual(decoded.source, .manual,
                       "CodingKeys and init(from:) must both carry `source`, or a manual record silently becomes browser-backed")
    }

    func testManualIsTheWireValueManual() throws {
        let data = try JSONEncoder().encode(account(source: .manual))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])

        XCTAssertEqual(json["source"] as? String, "manual",
                       "the wire value is what the Rust side reads")
    }

    func testALegacyRecordWithNoSourceDecodesAsBrowser() throws {
        let legacy = """
        {"id":"3F2504E0-4F89-11D3-9A0C-0305E82C3301","name":"person@example.com",
         "chromeProfilePath":"Default","plan":"Pro","status":"active"}
        """

        let decoded = try JSONDecoder().decode(Account.self, from: Data(legacy.utf8))

        XCTAssertEqual(decoded.source, .browser,
                       "every record written before this field existed is browser-backed")
    }
}
