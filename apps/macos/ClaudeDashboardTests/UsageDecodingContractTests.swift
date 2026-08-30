import XCTest
@testable import ClaudeDashboard

/// Drives `UsageData.decode` from `contract/cases/usage-decoding.json`.
/// The Rust suite reads the same file; changing a rule in one language without
/// the other turns one of the two suites red.
final class UsageDecodingContractTests: XCTestCase {

    private struct Case: Decodable {
        let name: String
        let input: JSONValue
        let expect: Expect

        struct Expect: Decodable {
            let fiveHourUtilization: Double
            let fiveHourResetsAtEpoch: Int?
            let sevenDayUtilization: Double
            let sevenDayResetsAtEpoch: Int?
            let fablePresent: Bool
            let fableUtilization: Double?
            let fableResetsAtEpoch: Int?

            enum CodingKeys: String, CodingKey {
                case fiveHourUtilization = "five_hour_utilization"
                case fiveHourResetsAtEpoch = "five_hour_resets_at_epoch"
                case sevenDayUtilization = "seven_day_utilization"
                case sevenDayResetsAtEpoch = "seven_day_resets_at_epoch"
                case fablePresent = "fable_present"
                case fableUtilization = "fable_utilization"
                case fableResetsAtEpoch = "fable_resets_at_epoch"
            }
        }
    }

    /// Minimal any-JSON box so `input` can be re-encoded and fed to the decoder
    /// under test without a second hand-written model of the API payload.
    private struct JSONValue: Decodable {
        let raw: Any

        init(from decoder: Decoder) throws {
            let c = try decoder.singleValueContainer()
            if let v = try? c.decode([String: JSONValue].self) {
                raw = v.mapValues(\.raw)
            } else if let v = try? c.decode([JSONValue].self) {
                raw = v.map(\.raw)
            } else if let v = try? c.decode(Double.self) {
                raw = v
            } else if let v = try? c.decode(Bool.self) {
                raw = v
            } else if let v = try? c.decode(String.self) {
                raw = v
            } else {
                raw = NSNull()
            }
        }
    }

    func testUsageDecodingCases() throws {
        let data = try ContractFixtures.data("cases/usage-decoding.json")
        let cases = try JSONDecoder().decode([Case].self, from: data)

        XCTAssertFalse(cases.isEmpty, "contract/cases/usage-decoding.json is empty")

        for c in cases {
            let inputData = try JSONSerialization.data(withJSONObject: c.input.raw)
            let usage = try UsageData.decode(from: inputData)

            XCTAssertEqual(usage.fiveHour.utilization, c.expect.fiveHourUtilization,
                           "five_hour utilization — case: \(c.name)")
            XCTAssertEqual(epoch(usage.fiveHour.resetsAt), c.expect.fiveHourResetsAtEpoch,
                           "five_hour resets_at — case: \(c.name)")
            XCTAssertEqual(usage.sevenDay.utilization, c.expect.sevenDayUtilization,
                           "seven_day utilization — case: \(c.name)")
            XCTAssertEqual(epoch(usage.sevenDay.resetsAt), c.expect.sevenDayResetsAtEpoch,
                           "seven_day resets_at — case: \(c.name)")

            XCTAssertEqual(usage.fable != nil, c.expect.fablePresent,
                           "fable presence — case: \(c.name)")
            XCTAssertEqual(usage.fable?.utilization, c.expect.fableUtilization,
                           "fable utilization — case: \(c.name)")
            XCTAssertEqual(epoch(usage.fable?.resetsAt), c.expect.fableResetsAtEpoch,
                           "fable resets_at — case: \(c.name)")
        }
    }

    /// Truncated Unix seconds — the contract's timestamp representation.
    private func epoch(_ date: Date?) -> Int? {
        date.map { Int($0.timeIntervalSince1970) }
    }
}
