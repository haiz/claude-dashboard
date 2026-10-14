import XCTest
@testable import ClaudeDashboard

final class AutoSwitchLogStoreTests: XCTestCase {
    private var dbPath: String!

    override func setUp() {
        super.setUp()
        dbPath = NSTemporaryDirectory() + "auto_switch_logs_\(UUID().uuidString).db"
    }

    override func tearDown() {
        try? FileManager.default.removeItem(atPath: dbPath)
        super.tearDown()
    }

    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    func testInsertThenFetchReturnsNewestFirstWithEveryField() async {
        let store = AutoSwitchLogStore(dbPath: dbPath)
        await store.insert(date: t0, outcome: .switched, fromName: "a@x.com", toName: "b@x.com",
                           reason: "5h 96% >= 95%, tier 1", detail: "b@x.com: 5h 10%, 7d 20%")
        await store.insert(date: t0.addingTimeInterval(60), outcome: .noCandidate, fromName: "b@x.com",
                           toName: nil, reason: "7d 99% >= 99%", detail: nil)

        let rows = await store.fetch(limit: 10)

        XCTAssertEqual(rows.count, 2)
        XCTAssertEqual(rows[0].outcome, .noCandidate)
        XCTAssertEqual(rows[0].date, t0.addingTimeInterval(60))
        XCTAssertNil(rows[0].toName)
        XCTAssertNil(rows[0].detail)
        XCTAssertEqual(rows[1], AutoSwitchLogEntry(
            id: rows[1].id, date: t0, outcome: .switched, fromName: "a@x.com", toName: "b@x.com",
            reason: "5h 96% >= 95%, tier 1", detail: "b@x.com: 5h 10%, 7d 20%"))
    }

    func testKeepsOnlyTheNewestRows() async {
        let store = AutoSwitchLogStore(dbPath: dbPath, maxEntries: 2)
        for i in 0..<3 {
            await store.insert(date: t0.addingTimeInterval(Double(i)), outcome: .switched,
                               fromName: "a", toName: "b", reason: "r\(i)", detail: nil)
        }
        let reasons = await store.fetch(limit: 10).map(\.reason)
        XCTAssertEqual(reasons, ["r2", "r1"])
    }

    func testClearEmptiesTheLog() async {
        let store = AutoSwitchLogStore(dbPath: dbPath)
        await store.insert(date: t0, outcome: .failed, fromName: "a", toName: "b", reason: "r", detail: "e")
        await store.clear()
        let rows = await store.fetch(limit: 10)
        XCTAssertTrue(rows.isEmpty)
    }

    func testOutcomeRawValuesAreFixed() {
        XCTAssertEqual(AutoSwitchOutcome.allCases.map(\.rawValue), [0, 1, 2])
    }
}
