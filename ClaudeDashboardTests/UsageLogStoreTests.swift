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

    func testSmartCompression_threeIdenticalValues_keepFirstAndLast() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 2, "Should keep first and last of identical streak")
        XCTAssertEqual(logs[0].utilization, 50.0, accuracy: 0.01)
        XCTAssertEqual(logs[1].utilization, 50.0, accuracy: 0.01)
        XCTAssertLessThanOrEqual(logs[0].recordedAt, logs[1].recordedAt)
    }

    func testSmartCompression_valueChanges_noCompression() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 30.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 70.0, isLimited: false)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 3, "All 3 should remain — values differ")
    }

    func testSmartCompression_fourIdenticalValues_keepFirstAndLast() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        for _ in 0..<4 {
            await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 60.0, isLimited: false)
            try? await Task.sleep(nanoseconds: 10_000_000)
        }

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 2, "Should keep only first and last of 4 identical values")
    }

    func testSmartCompression_plateauThenChange() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 40.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 40.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 40.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 60.0, isLimited: false)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 3, "2 from plateau + 1 new value")
        XCTAssertEqual(logs[0].utilization, 40.0, accuracy: 0.01)
        XCTAssertEqual(logs[1].utilization, 40.0, accuracy: 0.01)
        XCTAssertEqual(logs[2].utilization, 60.0, accuracy: 0.01)
    }

    func testDifferentWindowsNotCompressed() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)
        await store.record(accountId: accountId, window: .sevenDay, resetsAt: resetsAt, utilization: 50.0, isLimited: false)

        let logs5h = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        let logs7d = await store.logs(accountId: accountId, window: .sevenDay, from: nil, to: nil)
        XCTAssertEqual(logs5h.count, 1)
        XCTAssertEqual(logs7d.count, 1)
    }

    func testResetCycles() async {
        let accountId = UUID()
        let reset1 = Date().addingTimeInterval(3600)
        let reset2 = Date().addingTimeInterval(7200)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: reset1, utilization: 30.0, isLimited: false)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: reset1, utilization: 60.0, isLimited: false)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: reset2, utilization: 10.0, isLimited: false)

        let cycles = await store.resetCycles(accountId: accountId, window: .fiveHour)
        XCTAssertEqual(cycles.count, 2)
        XCTAssertEqual(cycles[0].peakUtilization, 10.0, accuracy: 0.01)
        XCTAssertEqual(cycles[0].dataPointCount, 1)
        XCTAssertEqual(cycles[1].peakUtilization, 60.0, accuracy: 0.01)
        XCTAssertEqual(cycles[1].dataPointCount, 2)
    }

    func testDeleteOlderThan() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)

        await store.deleteOlderThan(Date().addingTimeInterval(3600))

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 0)
    }

    func testIsLimitedFlag() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 100.0, isLimited: true)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 1)
        XCTAssertTrue(logs[0].isLimited)
    }
}
