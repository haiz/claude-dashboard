import XCTest
import SQLite3
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
        await store.insert(date: t0, trigger: .auto, outcome: .switched, fromName: "a@x.com", toName: "b@x.com",
                           reason: "5h 96% >= 95%, tier 1", detail: "b@x.com: 5h 10%, 7d 20%")
        await store.insert(date: t0.addingTimeInterval(60), trigger: .manual, outcome: .noCandidate, fromName: "b@x.com",
                           toName: nil, reason: "7d 99% >= 99%", detail: nil)

        let rows = await store.fetch(limit: 10)

        XCTAssertEqual(rows.count, 2)
        XCTAssertEqual(rows[0].outcome, .noCandidate)
        XCTAssertEqual(rows[0].trigger, .manual)
        XCTAssertEqual(rows[0].date, t0.addingTimeInterval(60))
        XCTAssertNil(rows[0].toName)
        XCTAssertNil(rows[0].detail)
        XCTAssertEqual(rows[1], AutoSwitchLogEntry(
            id: rows[1].id, date: t0, trigger: .auto, outcome: .switched, fromName: "a@x.com", toName: "b@x.com",
            reason: "5h 96% >= 95%, tier 1", detail: "b@x.com: 5h 10%, 7d 20%", target: .claudeCode))
    }

    func testKeepsOnlyTheNewestRows() async {
        let store = AutoSwitchLogStore(dbPath: dbPath, maxEntries: 2)
        for i in 0..<3 {
            await store.insert(date: t0.addingTimeInterval(Double(i)), trigger: .auto, outcome: .switched,
                               fromName: "a", toName: "b", reason: "r\(i)", detail: nil)
        }
        let reasons = await store.fetch(limit: 10).map(\.reason)
        XCTAssertEqual(reasons, ["r2", "r1"])
    }

    func testClearEmptiesTheLog() async {
        let store = AutoSwitchLogStore(dbPath: dbPath)
        await store.insert(date: t0, trigger: .auto, outcome: .failed, fromName: "a", toName: "b", reason: "r", detail: "e")
        await store.clear()
        let rows = await store.fetch(limit: 10)
        XCTAssertTrue(rows.isEmpty)
    }

    func testOutcomeAndTriggerRawValuesAreFixed() {
        XCTAssertEqual(AutoSwitchOutcome.allCases.map(\.rawValue), [0, 1, 2, 3])
        XCTAssertEqual(AutoSwitchTrigger.allCases.map(\.rawValue), [0, 1])
        XCTAssertEqual(AutoSwitchTrigger.allCases.map(\.label), ["Auto", "Manual"])
    }

    /// A database written before the `trig` column existed keeps its rows, read as Auto.
    func testOldDatabaseGainsTheTriggerColumnAndItsRowsReadAsAuto() async throws {
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(dbPath, &db), SQLITE_OK)
        sqlite3_exec(db, """
            CREATE TABLE auto_switch_logs (id INTEGER PRIMARY KEY AUTOINCREMENT, date REAL NOT NULL,
                outcome INTEGER NOT NULL, from_name TEXT, to_name TEXT, reason TEXT NOT NULL, detail TEXT);
            INSERT INTO auto_switch_logs (date, outcome, from_name, to_name, reason)
                VALUES (1800000000, 0, 'a', 'b', 'old');
            """, nil, nil, nil)
        sqlite3_close(db)

        let store = AutoSwitchLogStore(dbPath: dbPath)
        await store.insert(date: t0.addingTimeInterval(1), trigger: .manual, outcome: .switched,
                           fromName: "b", toName: "a", reason: "Manual switch", detail: nil)

        let rows = await store.fetch(limit: 10)
        XCTAssertEqual(rows.map(\.reason), ["Manual switch", "old"])
        XCTAssertEqual(rows.map(\.trigger), [.manual, .auto])
    }

    func testInsertDesktopTargetRow() async {
        let store = AutoSwitchLogStore(dbPath: dbPath)
        await store.insert(date: t0, trigger: .manual, outcome: .switched, fromName: "a", toName: "b",
                           reason: "Manual switch", detail: nil, target: .claudeDesktop)
        await store.insert(date: t0.addingTimeInterval(1), trigger: .auto, outcome: .skipped, fromName: "a",
                           toName: "b", reason: "r", detail: nil)
        let rows = await store.fetch(limit: 10)
        XCTAssertEqual(rows.map(\.target), [.claudeCode, .claudeDesktop])
        XCTAssertEqual(rows.map(\.outcome), [.skipped, .switched])
        XCTAssertEqual(SwitchTarget.allCases.map(\.rawValue), [0, 1])
        XCTAssertEqual(SwitchTarget.allCases.map(\.label), ["Claude Code", "Claude Desktop"])
    }

    /// A table from before the `target` column keeps its rows (read as Claude Code); opening the
    /// store again, with the column already there, does not fail.
    func testTargetColumnMigrationIdempotent() async throws {
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(dbPath, &db), SQLITE_OK)
        sqlite3_exec(db, """
            CREATE TABLE auto_switch_logs (id INTEGER PRIMARY KEY AUTOINCREMENT, date REAL NOT NULL,
                outcome INTEGER NOT NULL, from_name TEXT, to_name TEXT, reason TEXT NOT NULL, detail TEXT,
                trig INTEGER NOT NULL DEFAULT 0);
            INSERT INTO auto_switch_logs (date, outcome, from_name, to_name, reason, trig)
                VALUES (1800000000, 0, 'a', 'b', 'old', 1);
            """, nil, nil, nil)
        sqlite3_close(db)

        let first = AutoSwitchLogStore(dbPath: dbPath)
        await first.insert(date: t0, trigger: .manual, outcome: .switched, fromName: "b", toName: "a",
                           reason: "new", detail: nil, target: .claudeDesktop)
        let second = AutoSwitchLogStore(dbPath: dbPath)
        let rows = await second.fetch(limit: 10)
        XCTAssertEqual(rows.map(\.reason), ["new", "old"])
        XCTAssertEqual(rows.map(\.target), [.claudeDesktop, .claudeCode])
        XCTAssertEqual(rows.last?.trigger, .manual)
    }
}
