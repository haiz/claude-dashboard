import XCTest
@testable import ClaudeDashboard

/// Drives `ChartWindowSummary` from `contract/cases/chart-window-summary.json`, the fixture
/// the Rust `core::chart::window_summary` reads too.
final class ChartWindowSummaryContractTests: XCTestCase {

    private struct Case: Decodable {
        struct Expect: Decodable {
            let endIsNow: Bool
            let startPct: Int?
            let endPct: Int?
            enum CodingKeys: String, CodingKey {
                case endIsNow = "end_is_now", startPct = "start_pct", endPct = "end_pct"
            }
        }
        let name: String
        let window: String
        let from: Double
        let to: Double
        let now: Double
        let points: [[Double]]
        let expect: Expect
    }

    func testChartWindowSummaryCases() throws {
        let cases = try JSONDecoder().decode(
            [Case].self, from: ContractFixtures.data("cases/chart-window-summary.json"))
        XCTAssertFalse(cases.isEmpty)
        let accountId = UUID()
        for c in cases {
            let window: UsageWindow
            switch c.window {
            case "5h": window = .fiveHour
            case "7d": window = .sevenDay
            case "F": window = .fable
            default: return XCTFail("unknown window \(c.window)")
            }
            let logs = c.points.enumerated().map { i, p in
                UsageLogEntry(id: Int64(i), accountId: accountId, window: window,
                              resetsAt: Date(timeIntervalSince1970: 1e9),
                              recordedAt: Date(timeIntervalSince1970: p[0]),
                              utilization: p[1], isLimited: false)
            }
            let range = Date(timeIntervalSince1970: c.from)...Date(timeIntervalSince1970: c.to)
            let got = ChartWindowSummary.make(logs: logs, range: range,
                                              now: Date(timeIntervalSince1970: c.now), window: window)
            XCTAssertEqual(got.endIsNow, c.expect.endIsNow, "endIsNow: \(c.name)")
            XCTAssertEqual(got.startPct, c.expect.startPct, "startPct: \(c.name)")
            XCTAssertEqual(got.endPct, c.expect.endPct, "endPct: \(c.name)")
        }
    }

    func testTextNamesTheDayOnlyOffToday() {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = TimeZone(identifier: "UTC")!
        let now = Date(timeIntervalSince1970: 1_762_610_400)          // 8 Nov 2025 14:00 UTC
        let live = now.addingTimeInterval(-5 * 3600)...now
        let up = ChartWindowSummary(endIsNow: true, startPct: 2, endPct: 9)
        XCTAssertEqual(up.text(range: live, now: now, calendar: cal), "09:00 2% → now 9%")
        let flat = ChartWindowSummary(endIsNow: true, startPct: nil, endPct: nil)
        XCTAssertEqual(flat.text(range: live, now: now, calendar: cal), "09:00 → now")
        let past = now.addingTimeInterval(-30 * 3600)...now.addingTimeInterval(-25 * 3600)
        let old = ChartWindowSummary(endIsNow: false, startPct: nil, endPct: nil)
        XCTAssertEqual(old.text(range: past, now: now, calendar: cal), "7 Nov 08:00 → 7 Nov 13:00")
    }
}
