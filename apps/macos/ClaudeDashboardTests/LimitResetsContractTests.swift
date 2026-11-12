import XCTest
@testable import ClaudeDashboard

/// Drives `LimitResets` from `contract/cases/limit-resets.json`. The Rust suite
/// reads the same file.
final class LimitResetsContractTests: XCTestCase {

    private struct Summary: Decodable, Equatable {
        let count: Int
        let expiresAtEpoch: Int
        let urgent: Bool

        enum CodingKeys: String, CodingKey {
            case count, urgent
            case expiresAtEpoch = "expires_at_epoch"
        }
    }

    func testCases() throws {
        let data = try ContractFixtures.data("cases/limit-resets.json")
        let cases = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [[String: Any]])
        XCTAssertEqual(cases.count, 27)

        for c in cases {
            let name = c["name"] as? String ?? "?"
            let nowText = try XCTUnwrap(c["now"] as? String, name)
            let now = try XCTUnwrap(ISO8601DateFormatter().date(from: nowText), name)
            let expect = try XCTUnwrap(c["expect"] as? [String: Any], name)

            let summary = LimitResets.summarize(LimitResets.parseGrants(c["input"]), now: now)

            XCTAssertEqual(summary.full.map(Self.pack), try Self.expected(expect["full"]), "full - \(name)")
            XCTAssertEqual(summary.fable.map(Self.pack), try Self.expected(expect["fable"]), "fable - \(name)")
        }
    }

    func testNSNullInputGivesNoResets() {
        let summary = LimitResets.summarize(LimitResets.parseGrants(NSNull()), now: Date())
        XCTAssertNil(summary.full)
        XCTAssertNil(summary.fable)
    }

    func testFieldNameRoundTrips() {
        let name = LimitResets.fieldName
        XCTAssertEqual(name.count, 11)
        XCTAssertTrue(name.contains("_"))
        XCTAssertEqual(Data(name.utf8).base64EncodedString(), LimitResets.encodedFieldName)
    }

    func testCountSaturatesInsteadOfTrapping() throws {
        let end = Date(timeIntervalSince1970: 2_000_000_000)
        let grants = [
            ResetGrant(kind: .full, resetsLeft: Int.max, endsAt: end),
            ResetGrant(kind: .full, resetsLeft: 5, endsAt: end),
        ]
        let summary = LimitResets.summarize(grants, now: Date(timeIntervalSince1970: 0))
        XCTAssertEqual(summary.full?.count, Int.max)
    }

    private static func pack(_ s: ResetSummary) -> Summary {
        Summary(count: s.count, expiresAtEpoch: Int(s.expiresAt.timeIntervalSince1970), urgent: s.urgent)
    }

    private static func expected(_ raw: Any?) throws -> Summary? {
        guard let raw, !(raw is NSNull) else { return nil }
        let data = try JSONSerialization.data(withJSONObject: raw)
        return try JSONDecoder().decode(Summary.self, from: data)
    }
}
