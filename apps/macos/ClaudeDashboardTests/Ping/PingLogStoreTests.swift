import XCTest
@testable import ClaudeDashboard

final class PingLogStoreTests: XCTestCase {
    private var dbPath: String!

    override func setUp() {
        super.setUp()
        dbPath = NSTemporaryDirectory() + "ping_logs_\(UUID().uuidString).db"
    }

    override func tearDown() {
        try? FileManager.default.removeItem(atPath: dbPath)
        super.tearDown()
    }

    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    func testInsertThenFetchReturnsNewestFirstWithEveryField() async {
        let store = PingLogStore(dbPath: dbPath)
        let a = UUID()
        await store.insert(accountId: a, trigger: .manual, startedAt: t0, finishedAt: t0.addingTimeInterval(1.5),
                           result: PingResult(outcome: .ok, detail: nil))
        await store.insert(accountId: a, trigger: .auto, startedAt: t0.addingTimeInterval(10),
                           finishedAt: t0.addingTimeInterval(11),
                           result: PingResult(outcome: .failed, detail: "HTTP 429"))

        let rows = await store.fetch(limit: 10)

        XCTAssertEqual(rows.count, 2)
        XCTAssertEqual(rows[0].trigger, .auto)
        XCTAssertEqual(rows[0].outcome, .failed)
        XCTAssertEqual(rows[0].detail, "HTTP 429")
        XCTAssertEqual(rows[1].accountId, a)
        XCTAssertEqual(rows[1].outcome, .ok)
        XCTAssertNil(rows[1].detail)
        XCTAssertEqual(rows[1].finishedAt.timeIntervalSince1970, t0.timeIntervalSince1970 + 1.5, accuracy: 0.001)
    }

    func testLatestIsPerAccount() async {
        let store = PingLogStore(dbPath: dbPath)
        let a = UUID(), b = UUID()
        await store.insert(accountId: a, trigger: .manual, startedAt: t0, finishedAt: t0,
                           result: PingResult(outcome: .ok, detail: nil))
        await store.insert(accountId: b, trigger: .manual, startedAt: t0, finishedAt: t0,
                           result: PingResult(outcome: .blocked, detail: nil))

        let latestA = await store.latest(for: a)
        let latestNone = await store.latest(for: UUID())

        XCTAssertEqual(latestA?.outcome, .ok)
        XCTAssertNil(latestNone)
    }

    func testKeepsOnlyTheNewestMaxEntries() async {
        let store = PingLogStore(dbPath: dbPath, maxEntries: 3)
        let a = UUID()
        for i in 0..<5 {
            await store.insert(accountId: a, trigger: .auto, startedAt: t0.addingTimeInterval(Double(i)),
                               finishedAt: t0, result: PingResult(outcome: .ok, detail: "\(i)"))
        }

        let rows = await store.fetch(limit: 10)

        XCTAssertEqual(rows.map(\.detail), ["4", "3", "2"])
    }

    func testClearEmptiesTheLog() async {
        let store = PingLogStore(dbPath: dbPath)
        await store.insert(accountId: UUID(), trigger: .manual, startedAt: t0, finishedAt: t0,
                           result: PingResult(outcome: .ok, detail: nil))

        await store.clear()
        let rows = await store.fetch(limit: 10)

        XCTAssertTrue(rows.isEmpty)
    }

    func testRowsSurviveReopeningTheFile() async {
        let a = UUID()
        do {
            let store = PingLogStore(dbPath: dbPath)
            await store.insert(accountId: a, trigger: .manual, startedAt: t0, finishedAt: t0,
                               result: PingResult(outcome: .expired, detail: nil))
        }
        let reopened = PingLogStore(dbPath: dbPath)
        let latest = await reopened.latest(for: a)
        XCTAssertEqual(latest?.outcome, .expired)
    }

    func testCaptionForOkAndFailedPings() {
        let ok = PingLogEntry(id: 1, accountId: UUID(), trigger: .manual, startedAt: t0, finishedAt: t0,
                              outcome: .ok, detail: nil)
        let noted = PingLogEntry(id: 2, accountId: UUID(), trigger: .manual, startedAt: t0, finishedAt: t0,
                                 outcome: .ok, detail: "conversation not deleted")
        let expired = PingLogEntry(id: 3, accountId: UUID(), trigger: .auto, startedAt: t0, finishedAt: t0,
                                   outcome: .expired, detail: nil)
        let failed = PingLogEntry(id: 4, accountId: UUID(), trigger: .auto, startedAt: t0, finishedAt: t0,
                                  outcome: .failed, detail: "HTTP 429")
        let time: (Date) -> String = { _ in "10:44" }

        XCTAssertEqual(ok.caption(time: time), "Pinged at 10:44")
        XCTAssertEqual(noted.caption(time: time), "Pinged at 10:44 (conversation not deleted)")
        XCTAssertEqual(expired.caption(time: time), "Ping failed at 10:44: Session expired")
        XCTAssertEqual(failed.caption(time: time), "Ping failed at 10:44: Failed, HTTP 429")
    }
}
