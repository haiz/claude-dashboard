// ClaudeDashboardTests/CommandLogStoreTests.swift
import XCTest
import SQLite3
@testable import ClaudeDashboard

final class CommandLogStoreTests: XCTestCase {
    var store: CommandLogStore!
    var dbPath: String!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "test_cmdlog_\(UUID().uuidString).db"
        store = await CommandLogStore(dbPath: dbPath, maxEntries: 5)
    }

    override func tearDown() async throws {
        store = nil
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    func testDatabaseCreated() async {
        XCTAssertTrue(FileManager.default.fileExists(atPath: dbPath))
    }

    func testRecordAndRecent() async {
        let acct = UUID()
        let start = Date(timeIntervalSince1970: 1_000)
        let finish = Date(timeIntervalSince1970: 1_002)
        await store.record(accountId: acct, command: "echo hi", trigger: .manual,
                           startedAt: start, finishedAt: finish, status: .exited,
                           exitCode: 0, output: "hi\n")
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows[0].command, "echo hi")
        XCTAssertEqual(rows[0].exitCode, 0)
        XCTAssertEqual(rows[0].status, .exited)
        XCTAssertEqual(rows[0].output, "hi\n")
    }

    func testStatusAndOutputRoundTrip() async {
        await store.record(accountId: nil, command: "sleep 99", trigger: .manual,
                           startedAt: Date(), finishedAt: Date(), status: .timedOut,
                           exitCode: nil, output: "partial output")
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows[0].status, .timedOut)
        XCTAssertNil(rows[0].exitCode)
        XCTAssertEqual(rows[0].output, "partial output")
    }

    func testMigrationAddsColumnsToOldSchema() async {
        // Build an old-schema DB (no status/output columns) directly, then open with the store.
        let oldPath = NSTemporaryDirectory() + "old_schema_\(UUID().uuidString).db"
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(oldPath, &db), SQLITE_OK)
        let createOld = """
        CREATE TABLE command_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id TEXT, cmd TEXT NOT NULL, trig INTEGER NOT NULL,
            started INTEGER NOT NULL, finished INTEGER, exit_code INTEGER
        )
        """
        XCTAssertEqual(sqlite3_exec(db, createOld, nil, nil, nil), SQLITE_OK)
        let insertOld = "INSERT INTO command_logs (cmd, trig, started, finished, exit_code) VALUES ('legacy', 0, 1, 2, 0)"
        XCTAssertEqual(sqlite3_exec(db, insertOld, nil, nil, nil), SQLITE_OK)
        sqlite3_close(db)

        let migrated = await CommandLogStore(dbPath: oldPath)
        let rows = await migrated.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows[0].command, "legacy")
        XCTAssertEqual(rows[0].status, .exited)   // NULL status decodes to .exited
        XCTAssertNil(rows[0].output)
        try? FileManager.default.removeItem(atPath: oldPath)
    }

    func testNilAccountAndNilExitPersist() async {
        await store.record(accountId: nil, command: "broken", trigger: .autoEmpty,
                           startedAt: Date(), finishedAt: Date(), status: .exited,
                           exitCode: nil, output: nil)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertNil(rows[0].accountId)
        XCTAssertNil(rows[0].exitCode)
        XCTAssertEqual(rows[0].trigger, .autoEmpty)
    }

    func testNilFinishedAtPersists() async {
        await store.record(accountId: nil, command: "hung", trigger: .manual,
                           startedAt: Date(), finishedAt: nil, status: .exited,
                           exitCode: nil, output: nil)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertNil(rows[0].finishedAt)
    }

    func testRecentOrderingNewestFirst() async {
        for i in 0..<3 {
            await store.record(accountId: nil, command: "cmd\(i)", trigger: .manual,
                               startedAt: Date(), finishedAt: Date(), status: .exited,
                               exitCode: 0, output: nil)
        }
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.map(\.command), ["cmd2", "cmd1", "cmd0"])
    }

    func testPruneKeepsOnlyMaxEntries() async {
        for i in 0..<8 {
            await store.record(accountId: nil, command: "cmd\(i)", trigger: .manual,
                               startedAt: Date(), finishedAt: Date(), status: .exited,
                               exitCode: 0, output: nil)
        }
        let rows = await store.recent(limit: 100)
        XCTAssertEqual(rows.count, 5, "maxEntries=5 should cap stored rows")
        XCTAssertEqual(rows.first?.command, "cmd7")
        XCTAssertEqual(rows.last?.command, "cmd3")
    }

    func testClear() async {
        await store.record(accountId: nil, command: "x", trigger: .manual,
                           startedAt: Date(), finishedAt: Date(), status: .exited,
                           exitCode: 0, output: nil)
        await store.clear()
        let rows = await store.recent(limit: 10)
        XCTAssertTrue(rows.isEmpty)
    }
}
