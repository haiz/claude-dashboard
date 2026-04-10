// ClaudeDashboardTests/UsageLogStoreTests.swift
import XCTest
@testable import ClaudeDashboard

final class UsageLogStoreTests: XCTestCase {
    var store: UsageLogStore!
    var dbPath: String!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "test_usage_\(UUID().uuidString).db"
        store = await UsageLogStore(dbPath: dbPath)
    }

    override func tearDown() async throws {
        store = nil
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    func testDatabaseCreated() async {
        XCTAssertTrue(FileManager.default.fileExists(atPath: dbPath))
    }

    func testRecordAndQuerySingleEntry() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 45.5, isLimited: false)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 1)
        XCTAssertEqual(logs[0].accountId, accountId)
        XCTAssertEqual(logs[0].window, .fiveHour)
        XCTAssertEqual(logs[0].utilization, 45.5, accuracy: 0.01)
        XCTAssertFalse(logs[0].isLimited)
    }
}
