import XCTest
@testable import ClaudeDashboard

final class UsageDataTests: XCTestCase {

    func testDecodesFullResponse() throws {
        let json = """
        {
          "five_hour": { "utilization": 42.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
          "seven_day": { "utilization": 18.0, "resets_at": "2026-04-14T16:59:59.661657+00:00" },
          "seven_day_sonnet": { "utilization": 0.0, "resets_at": null },
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

    func testDecodesSevenDaySonnet() throws {
        let json = """
        {
          "five_hour": { "utilization": 42.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
          "seven_day": { "utilization": 18.0, "resets_at": "2026-04-14T16:59:59.661657+00:00" },
          "seven_day_sonnet": { "utilization": 25.0, "resets_at": "2026-04-12T10:00:00+00:00" }
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertNotNil(usage.sevenDaySonnet)
        XCTAssertEqual(usage.sevenDaySonnet?.utilization, 25.0)
        XCTAssertNotNil(usage.sevenDaySonnet?.resetsAt)
    }

    func testDecodesNullSevenDaySonnet() throws {
        let json = """
        {
          "five_hour": { "utilization": 0.0, "resets_at": null },
          "seven_day": { "utilization": 0.0, "resets_at": null },
          "seven_day_sonnet": null
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertNil(usage.sevenDaySonnet)
    }

    func testDecodesMissingSonnetField() throws {
        let json = """
        {
          "five_hour": { "utilization": 10.0, "resets_at": null },
          "seven_day": { "utilization": 5.0, "resets_at": null }
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertNil(usage.sevenDaySonnet)
    }
}
