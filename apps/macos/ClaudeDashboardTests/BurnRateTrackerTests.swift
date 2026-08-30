// ClaudeDashboardTests/BurnRateTrackerTests.swift
import XCTest
@testable import ClaudeDashboard

final class BurnRateTrackerTests: XCTestCase {
    var tracker: BurnRateTracker!
    var store: UsageLogStore!
    var dbPath: String!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "test_tracker_\(UUID().uuidString).db"
        store = await UsageLogStore(dbPath: dbPath)
        tracker = await BurnRateTracker(logStore: store)
    }

    override func tearDown() async throws {
        tracker = nil
        store = nil
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    func testFirstMeasurement_returnsNil() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 30.0, resetsAt: resetsAt
        )
        XCTAssertNil(result, "First measurement cannot compute speed")
    }

    func testSecondMeasurement_returnsRate() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 20.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 40.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(600)
        )

        XCTAssertNotNil(result)
        // Rate: 20% in 600s → 0.0333%/s → time to 100% from 40% = 60%/0.0333 = 1800s = 30min
        XCTAssertEqual(result!.level, 5) // < 30min → 🐆
        XCTAssertEqual(result!.animal, "🐆")
    }

    func testDifferentResetCycle_resetsHistory() async {
        let accountId = UUID()
        let reset1 = Date().addingTimeInterval(3600)
        let reset2 = Date().addingTimeInterval(7200)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 30.0, resetsAt: reset1, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 10.0, resetsAt: reset2, recordedAt: now.addingTimeInterval(300)
        )
        XCTAssertNil(result, "New cycle → reset history → nil")
    }

    func testUnchangedUtilization_gapOver5min_returnsNil() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 50.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 50.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(600)
        )
        XCTAssertNil(result, "Unchanged + gap >= 5min → nil")
    }

    func testUnchangedUtilization_gapUnder5min_withPrev_keepsPrevRate() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(18000)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 20.0, resetsAt: resetsAt, recordedAt: now
        )
        let result2 = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 40.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(600)
        )
        XCTAssertNotNil(result2)

        let result3 = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 40.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(720)
        )
        XCTAssertNotNil(result3, "Gap < 5min + has prev → keep rate")
        XCTAssertEqual(result3!.level, result2!.level)
    }

    func testUnchangedUtilization_gapUnder5min_noPrev_returnsNil() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 50.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 50.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(120)
        )
        XCTAssertNil(result, "Gap < 5min but no prev → nil")
    }

    func testUtilizationDecreases_resetsHistory() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 60.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 30.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(300)
        )
        XCTAssertNil(result, "Utilization decreased → reset → nil")
    }

    func testUtilization100_isLimitedLogged() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 80.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 100.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(300)
        )
        XCTAssertNotNil(result)
        XCTAssertEqual(result!.level, 5)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        let limitedLogs = logs.filter { $0.isLimited }
        XCTAssertFalse(limitedLogs.isEmpty, "100% utilization should be logged as limited")
    }

    func testSlowRate_level1() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(18000)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 5.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 10.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(1800)
        )
        XCTAssertNotNil(result)
        XCTAssertEqual(result!.level, 1)
        XCTAssertEqual(result!.animal, "🐌")
    }
}
