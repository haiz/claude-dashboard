import XCTest
@testable import ClaudeDashboard

/// Drives `SpeedSeries` and `PlanWeight` from `contract/cases/speed-series.json`, the fixture
/// the Rust `core::speed_series` reads too.
final class SpeedSeriesContractTests: XCTestCase {

    private struct Fixture: Decodable {
        struct Factor: Decodable { let plan: String; let factor: Double }
        struct SeriesCase: Decodable {
            let name: String
            let stepMin: Double
            let utcOffsetS: Double
            let from: Double
            let to: Double
            let plan: String
            let points: [[Double]]
            let expect: [[Double]]
            enum CodingKeys: String, CodingKey {
                case name, from, to, plan, points, expect
                case stepMin = "step_min", utcOffsetS = "utc_offset_s"
            }
        }
        struct TotalAccount: Decodable {
            let points: [[Double]]
            let lastEntry: Double
            enum CodingKeys: String, CodingKey { case points, lastEntry = "last_entry" }
        }
        struct TotalCase: Decodable {
            let name: String
            let now: Double
            let accounts: [TotalAccount]
            let expect: [[Double]]
        }
        let factors: [Factor]
        let series: [SeriesCase]
        let total: [TotalCase]
    }

    private func fixture() throws -> Fixture {
        try JSONDecoder().decode(Fixture.self, from: ContractFixtures.data("cases/speed-series.json"))
    }

    private func plan(_ raw: String) throws -> AccountPlan {
        try XCTUnwrap(AccountPlan(rawValue: raw), "unknown plan \(raw)")
    }

    private func date(_ t: Double) -> Date { Date(timeIntervalSince1970: t) }

    private func assertPoints(_ got: [SpeedPoint], _ expect: [[Double]], _ name: String,
                              file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(got.count, expect.count, "point count: \(name)", file: file, line: line)
        for (g, e) in zip(got, expect) {
            XCTAssertEqual(g.end.timeIntervalSince1970, e[0], "end: \(name)", file: file, line: line)
            XCTAssertEqual(g.value, e[1], accuracy: 1e-9, "value at \(e[0]): \(name)", file: file, line: line)
        }
    }

    func testFactors() throws {
        let f = try fixture()
        XCTAssertFalse(f.factors.isEmpty)
        for c in f.factors {
            XCTAssertEqual(PlanWeight.maxFactor(try plan(c.plan)), c.factor, "factor: \(c.plan)")
        }
    }

    func testSeriesCases() throws {
        let f = try fixture()
        XCTAssertFalse(f.series.isEmpty)
        let accountId = UUID()
        for c in f.series {
            let entries = c.points.enumerated().map { i, p in
                UsageLogEntry(id: Int64(i), accountId: accountId, window: .fiveHour,
                              resetsAt: date(1e9), recordedAt: date(p[0]),
                              utilization: p[1], isLimited: false)
            }
            let got = SpeedSeries.account(
                entries: entries, step: c.stepMin * 60, utcOffset: c.utcOffsetS,
                range: date(c.from)...date(c.to), factor: PlanWeight.maxFactor(try plan(c.plan)))
            assertPoints(got, c.expect, c.name)
        }
    }

    func testTotalCases() throws {
        let f = try fixture()
        XCTAssertFalse(f.total.isEmpty)
        for c in f.total {
            let inputs = c.accounts.map { a in
                SpeedSeries.AccountInput(
                    points: a.points.map { SpeedPoint(end: date($0[0]), value: $0[1]) },
                    lastEntry: date(a.lastEntry))
            }
            assertPoints(SpeedSeries.total(inputs, now: date(c.now)), c.expect, c.name)
        }
    }

    func testWeightsMatchAverageRate() {
        XCTAssertEqual(PlanWeight.weight(.pro), 1)
        XCTAssertEqual(PlanWeight.weight(.max5x), 5)
        XCTAssertEqual(PlanWeight.weight(.max20x), 20)
        XCTAssertEqual(PlanWeight.weight(.max200), 10)
        XCTAssertEqual(PlanWeight.weight(.team), 1)
        XCTAssertEqual(PlanWeight.weight(.enterprise), 1)
        XCTAssertEqual(PlanWeight.weight(.free), 1)
    }

    func testStepLabelsAndSeconds() {
        XCTAssertEqual(SpeedStep.allCases.map(\.label), ["6m", "10m", "20m", "30m"])
        XCTAssertEqual(SpeedStep.allCases.map(\.seconds), [360, 600, 1200, 1800])
    }
}
