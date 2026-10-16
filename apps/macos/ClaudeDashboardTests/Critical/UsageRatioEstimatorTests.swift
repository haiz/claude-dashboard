import XCTest
@testable import ClaudeDashboard

final class UsageRatioEstimatorTests: XCTestCase {
    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)
    private let a = UUID()
    private let b = UUID()

    private func row(_ account: UUID, _ window: UsageWindow, at minutes: Double,
                     resets: TimeInterval, _ u: Double) -> UsageLogEntry {
        UsageLogEntry(id: 0, accountId: account, window: window,
                      resetsAt: t0.addingTimeInterval(resets),
                      recordedAt: t0.addingTimeInterval(minutes * 60),
                      utilization: u, isLimited: false)
    }

    /// One account, one 5h window 0 -> 100 while 7d goes 10 -> 26: X = 16.
    func testSingleWindow() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0),
                    row(a, .fiveHour, at: 290, resets: 18_000, 100)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10),
                     row(a, .sevenDay, at: 290, resets: 500_000, 26)]
        XCTAssertEqual(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven),
                       UsageRatioEstimate(x: 16, sumFiveHour: 100, sumSevenDay: 16))
    }

    func testTwoAccountsArePooled() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0), row(a, .fiveHour, at: 100, resets: 18_000, 60),
                    row(b, .fiveHour, at: 0, resets: 19_000, 20), row(b, .fiveHour, at: 100, resets: 19_000, 60)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10), row(a, .sevenDay, at: 100, resets: 500_000, 20),
                     row(b, .sevenDay, at: 0, resets: 600_000, 40), row(b, .sevenDay, at: 100, resets: 600_000, 46)]
        let e = UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven)
        XCTAssertEqual(e?.sumFiveHour, 100)
        XCTAssertEqual(e?.sumSevenDay, 16)
        XCTAssertEqual(e?.x ?? 0, 16, accuracy: 1e-9)
    }

    func testJitteredResetsAtIsOneWindow() {
        let five = [row(a, .fiveHour, at: 0, resets: 17_999, 0),
                    row(a, .fiveHour, at: 100, resets: 18_000, 50),
                    row(a, .fiveHour, at: 200, resets: 17_999, 100)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10),
                     row(a, .sevenDay, at: 200, resets: 500_001, 26)]
        XCTAssertEqual(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven)?.sumFiveHour, 100)
    }

    func testWindowWithWeeklyResetInsideIsSkipped() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0), row(a, .fiveHour, at: 290, resets: 18_000, 100)]
        let seven = [row(a, .sevenDay, at: 0, resets: 1_000, 90), row(a, .sevenDay, at: 290, resets: 600_000, 5)]
        XCTAssertNil(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven))
    }

    func testSmallWindowsAreSkipped() {
        var five: [UsageLogEntry] = []
        for i in 0..<20 {   // 20 windows of +9 each: 180 total, but every one is under 10
            let resets = Double(i) * 18_000 + 18_000
            five += [row(a, .fiveHour, at: Double(i) * 300, resets: resets, 0),
                     row(a, .fiveHour, at: Double(i) * 300 + 100, resets: resets, 9)]
        }
        let seven = [row(a, .sevenDay, at: 0, resets: 900_000, 10)]
        XCTAssertNil(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven))
    }

    func testNotEnoughDataIsNil() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0), row(a, .fiveHour, at: 100, resets: 18_000, 99)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10), row(a, .sevenDay, at: 100, resets: 500_000, 26)]
        XCTAssertNil(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven))
    }

    /// Compression keeps only the first and last row of a run: the 7d reading at a time is the
    /// last row at or before it, even hours earlier. A 7d row 3 s after the 5h row still counts.
    func testSparseSevenDayRowsAndSampleTolerance() {
        let five = [row(a, .fiveHour, at: 600, resets: 36_000, 0), row(a, .fiveHour, at: 890, resets: 36_000, 100)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10),
                     row(a, .sevenDay, at: 890 + 0.05, resets: 500_000, 26)]
        XCTAssertEqual(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven)?.x ?? 0, 16, accuracy: 1e-9)
    }

    func testMissingSevenDayIsSkipped() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0), row(a, .fiveHour, at: 290, resets: 18_000, 100)]
        XCTAssertNil(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: []))
    }
}
