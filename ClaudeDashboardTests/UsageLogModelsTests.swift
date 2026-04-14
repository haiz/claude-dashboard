// ClaudeDashboardTests/UsageLogModelsTests.swift
import XCTest
@testable import ClaudeDashboard

final class UsageLogModelsTests: XCTestCase {
    func testBurnRateLevel1_over5hours() {
        let result = BurnRateResult.fromProjectedTime(6 * 3600) // 6 hours
        XCTAssertEqual(result.level, 1)
        XCTAssertEqual(result.animal, "🐌")
    }

    func testBurnRateLevel2_3to5hours() {
        let result = BurnRateResult.fromProjectedTime(4 * 3600) // 4 hours
        XCTAssertEqual(result.level, 2)
        XCTAssertEqual(result.animal, "🐢")
    }

    func testBurnRateLevel3_1point5to3hours() {
        let result = BurnRateResult.fromProjectedTime(2 * 3600) // 2 hours
        XCTAssertEqual(result.level, 3)
        XCTAssertEqual(result.animal, "🐇")
    }

    func testBurnRateLevel4_30mto1point5hours() {
        let result = BurnRateResult.fromProjectedTime(1 * 3600) // 1 hour
        XCTAssertEqual(result.level, 4)
        XCTAssertEqual(result.animal, "🐎")
    }

    func testBurnRateLevel5_under30min() {
        let result = BurnRateResult.fromProjectedTime(15 * 60) // 15 min
        XCTAssertEqual(result.level, 5)
        XCTAssertEqual(result.animal, "🐆")
    }

    func testBurnRateBoundary_exactly5hours() {
        let result = BurnRateResult.fromProjectedTime(5 * 3600)
        XCTAssertEqual(result.level, 2) // 5h is not > 5h, so level 2
    }

    func testBurnRateBoundary_exactly30min() {
        let result = BurnRateResult.fromProjectedTime(30 * 60)
        XCTAssertEqual(result.level, 5) // 0.5h is not > 0.5h, so level 5
    }

    func testUsageWindowLabels() {
        XCTAssertEqual(UsageWindow.fiveHour.label, "5h")
        XCTAssertEqual(UsageWindow.sevenDay.label, "7d")
        XCTAssertEqual(UsageWindow.sonnet.label, "S")
    }

    func testUsageWindowRawValues() {
        XCTAssertEqual(UsageWindow.fiveHour.rawValue, 0)
        XCTAssertEqual(UsageWindow.sevenDay.rawValue, 1)
        XCTAssertEqual(UsageWindow.sonnet.rawValue, 2)
    }
}

// MARK: - Reset Transition Tests

final class ResetTransitionTests: XCTestCase {

    // Helper to create a log entry with minimal boilerplate
    private func makeLog(
        id: Int64 = 0,
        accountId: UUID = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!,
        window: UsageWindow = .fiveHour,
        resetsAt: Date,
        recordedAt: Date,
        utilization: Double,
        isLimited: Bool = false
    ) -> UsageLogEntry {
        UsageLogEntry(
            id: id,
            accountId: accountId,
            window: window,
            resetsAt: resetsAt,
            recordedAt: recordedAt,
            utilization: utilization,
            isLimited: isLimited
        )
    }

    // MARK: - Basic reset injection

    func testInjectsTwoSyntheticPointsAtResetBoundary() {
        let resetTime = Date(timeIntervalSince1970: 1000)
        let newResetTime = Date(timeIntervalSince1970: 19000)

        let logs = [
            makeLog(id: 1, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 77.0),
            makeLog(id: 2, resetsAt: newResetTime,
                    recordedAt: Date(timeIntervalSince1970: 1100),
                    utilization: 6.0),
        ]

        let result = logs.withResetTransitions()

        // 2 original + 2 synthetic = 4
        XCTAssertEqual(result.count, 4)

        // Point 0: original pre-reset log
        XCTAssertEqual(result[0].id, 1)
        XCTAssertEqual(result[0].utilization, 77.0)

        // Point 1: synthetic hold (resetsAt - 1s, keeps old utilization)
        XCTAssertEqual(result[1].recordedAt, Date(timeIntervalSince1970: 999))
        XCTAssertEqual(result[1].utilization, 77.0)
        XCTAssertEqual(result[1].resetsAt, resetTime)

        // Point 2: synthetic drop (at resetsAt, 0%)
        XCTAssertEqual(result[2].recordedAt, resetTime)
        XCTAssertEqual(result[2].utilization, 0.0)
        XCTAssertEqual(result[2].resetsAt, newResetTime)

        // Point 3: original post-reset log
        XCTAssertEqual(result[3].id, 2)
        XCTAssertEqual(result[3].utilization, 6.0)
    }

    // MARK: - No reset (same resetsAt)

    func testNoInjectionWhenSameResetsAt() {
        let resetTime = Date(timeIntervalSince1970: 2000)

        let logs = [
            makeLog(id: 1, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 30.0),
            makeLog(id: 2, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 1200),
                    utilization: 45.0),
        ]

        let result = logs.withResetTransitions()
        XCTAssertEqual(result.count, 2)
        XCTAssertEqual(result[0].id, 1)
        XCTAssertEqual(result[1].id, 2)
    }

    // MARK: - Multi-account: only resetting account gets synthetic points

    func testMultiAccountOnlyResettingAccountGetsPoints() {
        let account1 = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!
        let account2 = UUID(uuidString: "00000000-0000-0000-0000-000000000002")!
        let resetTime = Date(timeIntervalSince1970: 1000)
        let newResetTime = Date(timeIntervalSince1970: 19000)

        let logs = [
            // Account 1: has reset
            makeLog(id: 1, accountId: account1, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 80.0),
            makeLog(id: 2, accountId: account1, resetsAt: newResetTime,
                    recordedAt: Date(timeIntervalSince1970: 1100),
                    utilization: 5.0),
            // Account 2: no reset
            makeLog(id: 3, accountId: account2, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 950),
                    utilization: 20.0),
            makeLog(id: 4, accountId: account2, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 1050),
                    utilization: 35.0),
        ]

        let result = logs.withResetTransitions()

        // Account 1: 2 original + 2 synthetic = 4
        // Account 2: 2 original, no synthetic
        XCTAssertEqual(result.count, 6)

        let account1Logs = result.filter { $0.accountId == account1 }
        let account2Logs = result.filter { $0.accountId == account2 }
        XCTAssertEqual(account1Logs.count, 4)
        XCTAssertEqual(account2Logs.count, 2)
    }

    // MARK: - Edge case: resetsAt <= recordedAt → skip

    func testSkipsWhenResetsAtBeforeRecordedAt() {
        let logs = [
            makeLog(id: 1,
                    resetsAt: Date(timeIntervalSince1970: 800),
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 50.0),
            makeLog(id: 2,
                    resetsAt: Date(timeIntervalSince1970: 5000),
                    recordedAt: Date(timeIntervalSince1970: 1100),
                    utilization: 10.0),
        ]

        let result = logs.withResetTransitions()

        // resetsAt(800) <= recordedAt(900), so skip injection
        XCTAssertEqual(result.count, 2)
    }

    // MARK: - Synthetic IDs are negative

    func testSyntheticEntriesHaveNegativeIds() {
        let resetTime = Date(timeIntervalSince1970: 1000)
        let newResetTime = Date(timeIntervalSince1970: 19000)

        let logs = [
            makeLog(id: 1, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 60.0),
            makeLog(id: 2, resetsAt: newResetTime,
                    recordedAt: Date(timeIntervalSince1970: 1100),
                    utilization: 10.0),
        ]

        let result = logs.withResetTransitions()
        let syntheticEntries = result.filter { $0.id < 0 }

        XCTAssertEqual(syntheticEntries.count, 2)
        // Verify all IDs are unique
        let ids = Set(result.map { $0.id })
        XCTAssertEqual(ids.count, result.count)
    }

    // MARK: - Edge case: resetsAt - 1s overlaps recordedAt → skip point 1 only

    func testSkipsSyntheticHoldWhenTooCloseToRecordedAt() {
        let resetTime = Date(timeIntervalSince1970: 901)

        let logs = [
            makeLog(id: 1,
                    resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 50.0),
            makeLog(id: 2,
                    resetsAt: Date(timeIntervalSince1970: 19000),
                    recordedAt: Date(timeIntervalSince1970: 1100),
                    utilization: 10.0),
        ]

        let result = logs.withResetTransitions()

        // resetsAt - 1s = 900, which == recordedAt of prev log → skip hold point
        // But drop point at resetsAt(901) is still valid
        // So: 2 original + 1 synthetic (drop only) = 3
        XCTAssertEqual(result.count, 3)

        // The synthetic point should be the drop at 0%
        let synthetic = result.filter { $0.id < 0 }
        XCTAssertEqual(synthetic.count, 1)
        XCTAssertEqual(synthetic[0].utilization, 0.0)
        XCTAssertEqual(synthetic[0].recordedAt, resetTime)
    }

    // MARK: - Edge case: resetsAt > current.recordedAt → skip

    func testSkipsWhenResetsAtAfterNextRecordedAt() {
        let logs = [
            makeLog(id: 1,
                    resetsAt: Date(timeIntervalSince1970: 1200),
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 50.0),
            makeLog(id: 2,
                    resetsAt: Date(timeIntervalSince1970: 5000),
                    recordedAt: Date(timeIntervalSince1970: 1100),
                    utilization: 10.0),
        ]

        let result = logs.withResetTransitions()

        // resetsAt(1200) > current.recordedAt(1100), so skip injection
        XCTAssertEqual(result.count, 2)
    }

    // MARK: - Empty and single-element arrays

    func testEmptyArrayReturnsEmpty() {
        let logs: [UsageLogEntry] = []
        XCTAssertEqual(logs.withResetTransitions().count, 0)
    }

    func testSingleElementReturnsUnchanged() {
        let logs = [
            makeLog(id: 1,
                    resetsAt: Date(timeIntervalSince1970: 1000),
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 50.0),
        ]

        let result = logs.withResetTransitions()
        XCTAssertEqual(result.count, 1)
        XCTAssertEqual(result[0].id, 1)
    }

    // MARK: - Result is sorted by recordedAt

    func testResultIsSortedByRecordedAt() {
        let account1 = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!
        let account2 = UUID(uuidString: "00000000-0000-0000-0000-000000000002")!
        let resetTime = Date(timeIntervalSince1970: 1000)
        let newResetTime = Date(timeIntervalSince1970: 19000)

        let logs = [
            makeLog(id: 1, accountId: account1, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 900),
                    utilization: 77.0),
            makeLog(id: 2, accountId: account2, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 950),
                    utilization: 40.0),
            makeLog(id: 3, accountId: account1, resetsAt: newResetTime,
                    recordedAt: Date(timeIntervalSince1970: 1100),
                    utilization: 6.0),
            makeLog(id: 4, accountId: account2, resetsAt: resetTime,
                    recordedAt: Date(timeIntervalSince1970: 1150),
                    utilization: 55.0),
        ]

        let result = logs.withResetTransitions()

        // Verify monotonically non-decreasing recordedAt
        for i in 1..<result.count {
            XCTAssertLessThanOrEqual(
                result[i - 1].recordedAt, result[i].recordedAt,
                "Entry at index \(i) is out of order"
            )
        }
    }
}
