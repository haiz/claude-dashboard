// ClaudeDashboardTests/CommandLogStoreTests.swift
import XCTest
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
                           startedAt: start, finishedAt: finish, exitCode: 0)

        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows[0].accountId, acct)
        XCTAssertEqual(rows[0].command, "echo hi")
        XCTAssertEqual(rows[0].trigger, .manual)
        XCTAssertEqual(rows[0].startedAt.timeIntervalSince1970, 1_000, accuracy: 0.5)
        XCTAssertEqual(rows[0].finishedAt?.timeIntervalSince1970 ?? -1, 1_002, accuracy: 0.5)
        XCTAssertEqual(rows[0].exitCode, 0)
    }

    func testNilAccountAndNilExitPersist() async {
        await store.record(accountId: nil, command: "broken", trigger: .autoEmpty,
                           startedAt: Date(), finishedAt: Date(), exitCode: nil)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertNil(rows[0].accountId)
        XCTAssertNil(rows[0].exitCode)
        XCTAssertEqual(rows[0].trigger, .autoEmpty)
    }

    func testNilFinishedAtPersists() async {
        await store.record(accountId: nil, command: "hung", trigger: .manual,
                           startedAt: Date(), finishedAt: nil, exitCode: nil)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertNil(rows[0].finishedAt)
    }

    func testRecentOrderingNewestFirst() async {
        for i in 0..<3 {
            await store.record(accountId: nil, command: "cmd\(i)", trigger: .manual,
                               startedAt: Date(), finishedAt: Date(), exitCode: 0)
        }
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.map(\.command), ["cmd2", "cmd1", "cmd0"])
    }

    func testPruneKeepsOnlyMaxEntries() async {
        for i in 0..<8 {
            await store.record(accountId: nil, command: "cmd\(i)", trigger: .manual,
                               startedAt: Date(), finishedAt: Date(), exitCode: 0)
        }
        let rows = await store.recent(limit: 100)
        XCTAssertEqual(rows.count, 5, "maxEntries=5 should cap stored rows")
        XCTAssertEqual(rows.first?.command, "cmd7")
        XCTAssertEqual(rows.last?.command, "cmd3")
    }

    func testClear() async {
        await store.record(accountId: nil, command: "x", trigger: .manual,
                           startedAt: Date(), finishedAt: Date(), exitCode: 0)
        await store.clear()
        let rows = await store.recent(limit: 10)
        XCTAssertTrue(rows.isEmpty)
    }
}
