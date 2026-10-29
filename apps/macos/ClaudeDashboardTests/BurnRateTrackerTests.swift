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

    /// The API reports a window boundary with jitter (`16:59:59.661633` on one call,
    /// `16:59:59.661657` on the next, sometimes whole seconds apart): still one cycle.
    func testJitteredResetsAt_withinTolerance_keepsCycle() async {
        for jitter in [0.000_024, 30.0, -60.0] {
            let accountId = UUID()
            let resetsAt = Date().addingTimeInterval(3600)
            let now = Date()

            let _ = await tracker.record(
                accountId: accountId, window: .fiveHour,
                utilization: 20.0, resetsAt: resetsAt, recordedAt: now
            )
            let result = await tracker.record(
                accountId: accountId, window: .fiveHour,
                utilization: 40.0, resetsAt: resetsAt.addingTimeInterval(jitter),
                recordedAt: now.addingTimeInterval(600)
            )
            XCTAssertEqual(result?.level, 5, "jitter \(jitter)s is the same cycle")
        }
    }

    func testResetsAtBeyondTolerance_resetsHistory() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 20.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 40.0, resetsAt: resetsAt.addingTimeInterval(61),
            recordedAt: now.addingTimeInterval(600)
        )
        XCTAssertNil(result, "More than 60s apart is a new cycle")
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

    private struct Step: Decodable {
        let utilization: Double
        let resetsAt: Double
        let at: Double
        let expectLevel: Int?
        let expectProjectedSeconds: Double?

        enum CodingKeys: String, CodingKey {
            case utilization, at
            case resetsAt = "resets_at"
            case expectLevel = "expect_level"
            case expectProjectedSeconds = "expect_projected_seconds"
        }
    }

    private struct TrackerCase: Decodable {
        let name: String
        let steps: [Step]
    }

    private struct TrackerFixture: Decodable {
        let cases: [TrackerCase]
    }

    /// Replays `contract/cases/burn-rate-tracker.json`, one fresh account per case.
    func testTrackerContractCases() async throws {
        let fixture = try JSONDecoder().decode(
            TrackerFixture.self, from: ContractFixtures.data("cases/burn-rate-tracker.json"))
        XCTAssertFalse(fixture.cases.isEmpty)
        for c in fixture.cases {
            let accountId = UUID()
            for (i, step) in c.steps.enumerated() {
                let r = await tracker.record(
                    accountId: accountId, window: .fiveHour, utilization: step.utilization,
                    resetsAt: Date(timeIntervalSince1970: step.resetsAt),
                    recordedAt: Date(timeIntervalSince1970: step.at))
                XCTAssertEqual(r?.level, step.expectLevel, "level - case: \(c.name), step \(i)")
                if let expected = step.expectProjectedSeconds {
                    XCTAssertEqual(r?.projectedTime ?? .nan, expected, accuracy: 1e-6,
                                   "projected - case: \(c.name), step \(i)")
                }
            }
        }
    }

    /// A skipped (too close) measurement is still written to the usage log.
    func testEveryCallIsLogged_evenWhenSkipped() async {
        let accountId = UUID()
        let resetsAt = Date(timeIntervalSince1970: 100_000)
        for (util, at) in [(10.0, 0.0), (11.0, 10.0), (12.0, 20.0)] {
            _ = await tracker.record(accountId: accountId, window: .fiveHour, utilization: util,
                                     resetsAt: resetsAt, recordedAt: Date(timeIntervalSince1970: at))
        }
        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 3)
    }
}
