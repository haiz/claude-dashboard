import XCTest
@testable import ClaudeDashboard

final class UsageDataTests: XCTestCase {

    func testDecodesFullResponse() throws {
        let json = """
        {
          "five_hour": { "utilization": 42.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
          "seven_day": { "utilization": 18.0, "resets_at": "2026-04-14T16:59:59.661657+00:00" },
          "seven_day_opus": null,
          "seven_day_oauth_apps": null,
          "seven_day_cowork": null,
          "extra_usage": null
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertEqual(usage.fiveHour.utilization, 42.0)
        XCTAssertNotNil(usage.fiveHour.resetsAt)
        XCTAssertEqual(usage.sevenDay.utilization, 18.0)
        XCTAssertNotNil(usage.sevenDay.resetsAt)
    }

    func testDecodesResponseWithNullResetsAt() throws {
        let json = """
        {
          "five_hour": { "utilization": 0.0, "resets_at": null },
          "seven_day": { "utilization": 0.0, "resets_at": null }
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertEqual(usage.fiveHour.utilization, 0.0)
        XCTAssertNil(usage.fiveHour.resetsAt)
        XCTAssertEqual(usage.sevenDay.utilization, 0.0)
        XCTAssertNil(usage.sevenDay.resetsAt)
    }

    func testDecodesDateWithFractionalSeconds() throws {
        let json = """
        {
          "five_hour": { "utilization": 10.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
          "seven_day": { "utilization": 5.0, "resets_at": "2026-04-14T16:59:59+00:00" }
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertNotNil(usage.fiveHour.resetsAt)
        XCTAssertNotNil(usage.sevenDay.resetsAt)
    }

    func testIgnoresUnknownFields() throws {
        // Claude's usage endpoint carries extra windows we don't model (e.g. the
        // former seven_day_sonnet). Decoding must ignore unknown keys.
        let json = """
        {
          "five_hour": { "utilization": 10.0, "resets_at": null },
          "seven_day": { "utilization": 5.0, "resets_at": null },
          "seven_day_sonnet": { "utilization": 25.0, "resets_at": "2026-04-12T10:00:00+00:00" }
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertEqual(usage.fiveHour.utilization, 10.0)
        XCTAssertEqual(usage.sevenDay.utilization, 5.0)
    }

    // The usage endpoint has no `seven_day_fable` field. Fable is reported only
    // as a `weekly_scoped` entry inside the `limits` array.
    func testDecodesFableFromLimits() throws {
        let json = """
        {
          "five_hour": { "utilization": 9.0, "resets_at": null },
          "seven_day": { "utilization": 66.0, "resets_at": null },
          "limits": [
            { "kind": "session", "group": "session", "percent": 9, "resets_at": null, "scope": null },
            { "kind": "weekly_all", "group": "weekly", "percent": 66, "resets_at": null, "scope": null },
            { "kind": "weekly_scoped", "group": "weekly", "percent": 100,
              "resets_at": "2026-07-03T17:00:00.283013+00:00",
              "scope": { "model": { "id": null, "display_name": "Fable" }, "surface": null } }
          ]
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertNotNil(usage.fable)
        XCTAssertEqual(usage.fable?.utilization, 100.0)
        XCTAssertNotNil(usage.fable?.resetsAt)
    }

    func testFableNilWhenNoScopedLimit() throws {
        let json = """
        {
          "five_hour": { "utilization": 9.0, "resets_at": null },
          "seven_day": { "utilization": 66.0, "resets_at": null },
          "limits": [
            { "kind": "session", "group": "session", "percent": 9, "resets_at": null, "scope": null },
            { "kind": "weekly_all", "group": "weekly", "percent": 66, "resets_at": null, "scope": null }
          ]
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertNil(usage.fable)
    }

    func testFableNilWhenNoLimitsArray() throws {
        let json = """
        {
          "five_hour": { "utilization": 9.0, "resets_at": null },
          "seven_day": { "utilization": 66.0, "resets_at": null }
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertNil(usage.fable)
    }
}
