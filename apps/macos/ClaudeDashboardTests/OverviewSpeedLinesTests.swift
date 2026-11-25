import XCTest
@testable import ClaudeDashboard

/// `OverviewSpeedLines.make`: per-account Max-equivalent lines, the Total, and the y-axis top.
final class OverviewSpeedLinesTests: XCTestCase {

    private let maxId = UUID()
    private let proId = UUID()

    private func date(_ t: Double) -> Date { Date(timeIntervalSince1970: t) }

    private func entries(_ id: UUID, _ points: [(Double, Double)]) -> [UsageLogEntry] {
        points.enumerated().map { i, p in
            UsageLogEntry(id: Int64(i), accountId: id, window: .fiveHour, resetsAt: date(100_000),
                          recordedAt: date(p.0), utilization: p.1, isLimited: false)
        }
    }

    /// Fixture case 1's points (one 1% rise in [0, 360)), plus a last entry at 9990.
    private func oneRise(_ id: UUID) -> [UsageLogEntry] {
        entries(id, [(0, 5), (120, 5), (240, 6), (720, 6), (9990, 6)])
    }

    private func make(_ logs: [UsageLogEntry], selected: Set<UUID>? = nil,
                      step: SpeedStep = .six, range: ClosedRange<Date>? = nil,
                      now: Double = 10_000) -> OverviewSpeedLines {
        OverviewSpeedLines.make(logs: logs, plans: [maxId: .max200, proId: .pro], order: [maxId, proId],
                                selected: selected ?? [maxId, proId], step: step,
                                range: range ?? date(0)...date(720), now: date(now), utcOffset: 0)
    }

    private func assertPoints(_ got: [SpeedPoint], _ expect: [(Double, Double)],
                              file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(got.count, expect.count, "point count", file: file, line: line)
        for (g, e) in zip(got, expect) {
            XCTAssertEqual(g.end.timeIntervalSince1970, e.0, file: file, line: line)
            XCTAssertEqual(g.value, e.1, accuracy: 1e-9, file: file, line: line)
        }
    }

    func testScalesAndTotals() {
        let lines = make(oneRise(maxId) + oneRise(proId))
        XCTAssertEqual(lines.accounts.map(\.id), [maxId, proId])
        assertPoints(lines.accounts[0].points, [(360, 10.0), (720, 0.0)])
        assertPoints(lines.accounts[1].points, [(360, 1.0), (720, 0.0)])
        assertPoints(lines.total, [(360, 11.0), (720, 0.0)])
    }

    func testAccountWithoutLogsIsSkipped() {
        let lines = make(oneRise(maxId))
        XCTAssertEqual(lines.accounts.map(\.id), [maxId])
        XCTAssertEqual(lines.total, lines.accounts[0].points)
    }

    func testNoSelectionGivesNoLines() {
        let lines = make(oneRise(maxId) + oneRise(proId), selected: [])
        XCTAssertTrue(lines.accounts.isEmpty)
        XCTAssertTrue(lines.total.isEmpty)
        XCTAssertEqual(lines.yMax, 10)
    }

    func testYAxisMaxFloorsAtTen() {
        let lines = make(entries(maxId, [(0, 5), (720, 5)]))
        XCTAssertEqual(lines.accounts.first?.points.map(\.value), [0, 0])
        XCTAssertEqual(lines.yMax, 10)
    }

    func testYAxisMaxRoundsUp() {
        // A 2.3% rise in one 6-min bucket on Max reads 23 %/h; 23 x 1.1 = 25.3 -> 30.
        let lines = make(entries(maxId, [(0, 0), (120, 0), (240, 2.3), (720, 2.3)]))
        XCTAssertEqual(lines.accounts.first?.points.first?.value ?? 0, 23, accuracy: 1e-9)
        XCTAssertEqual(lines.yMax, 30)
    }

    func testSevenDaysTenAccountsIsFast() {
        let days = 7.0 * 86400
        let ids = (0..<10).map { _ in UUID() }
        var logs: [UsageLogEntry] = []
        for id in ids {
            logs += stride(from: 0.0, through: days, by: 120).enumerated().map { i, t in
                UsageLogEntry(id: Int64(i), accountId: id, window: .fiveHour, resetsAt: date(days + 3600),
                              recordedAt: date(t), utilization: (t / 600).rounded(.down), isLimited: false)
            }
        }
        let plans = Dictionary(uniqueKeysWithValues: ids.map { ($0, AccountPlan.max200) })

        let start = Date()
        let lines = OverviewSpeedLines.make(logs: logs, plans: plans, order: ids, selected: Set(ids),
                                            step: .six, range: date(0)...date(days),
                                            now: date(days), utcOffset: 0)
        let elapsed = Date().timeIntervalSince(start)

        XCTAssertEqual(lines.accounts.count, 10)
        XCTAssertFalse(lines.total.isEmpty)
        XCTAssertLessThan(elapsed, 1.0)
    }
}
