import XCTest
@testable import ClaudeDashboard

final class CriticalPlannerTests: XCTestCase {
    private let vn = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
    private let ny = TimeZone(identifier: "America/New_York")!

    /// A wall-clock time in `zone`.
    private func at(_ y: Int, _ m: Int, _ d: Int, _ h: Int, _ min: Int = 0, in zone: TimeZone) -> Date {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = zone
        return cal.date(from: DateComponents(year: y, month: m, day: d, hour: h, minute: min))!
    }

    private var quietVN: QuietHours { QuietHours(startHour: 0, endHour: 7, timeZone: vn) }

    private func params(x: Double = 16, quiet: QuietHours?) -> CriticalParameters {
        CriticalParameters(x: x, pace: 50, highPace: 80, quiet: quiet)
    }

    private func usage(seven: Double, resets: Date?) -> UsageData {
        UsageData(fiveHour: UsageLimit(utilization: 0, resetsAt: nil),
                  sevenDay: UsageLimit(utilization: seven, resetsAt: resets))
    }

    // MARK: activeHours

    func testNoQuietHoursCountsEverything() {
        let now = at(2026, 10, 16, 10, in: vn)
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: now.addingTimeInterval(48 * 3600), quiet: nil), 48, accuracy: 1e-9)
    }

    func testTwoDaysLoseTwoQuietBlocks() {
        let now = at(2026, 10, 16, 10, in: vn)
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: now.addingTimeInterval(48 * 3600), quiet: quietVN), 34, accuracy: 1e-9)
    }

    func testStartingInsideQuietHours() {
        let now = at(2026, 10, 16, 3, in: vn)     // 4h of quiet left, then 07:00 to 12:00
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: at(2026, 10, 16, 12, in: vn), quiet: quietVN), 5, accuracy: 1e-9)
    }

    func testQuietHoursCrossingMidnight() {
        let q = QuietHours(startHour: 22, endHour: 6, timeZone: vn)
        let now = at(2026, 10, 16, 20, in: vn)
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: at(2026, 10, 17, 8, in: vn), quiet: q), 4, accuracy: 1e-9)
    }

    func testQuietHoursUseTheirOwnTimeZone() {
        // 10:00 to 20:00 Vietnam time is 23:00 to 09:00 New York time (EDT). With quiet hours
        // 00:00 to 07:00 *New York*, 7 of the 10 hours are quiet. An implementation that reads
        // quiet hours in the Mac's zone (Vietnam on this machine) would find none.
        let q = QuietHours(startHour: 0, endHour: 7, timeZone: ny)
        let now = at(2026, 10, 16, 10, in: vn)
        let hours = CriticalPlanner.activeHours(from: now, to: at(2026, 10, 16, 20, in: vn), quiet: q)
        XCTAssertEqual(hours, 3, accuracy: 1e-9)
    }

    func testDaylightSavingDayInNewYork() {
        // 2026-11-01 is 25h long in New York; quiet 00:00 to 07:00 on that day is 8 real hours.
        let q = QuietHours(startHour: 0, endHour: 7, timeZone: ny)
        let start = at(2026, 10, 31, 12, in: ny)
        let end = at(2026, 11, 1, 12, in: ny)
        XCTAssertEqual(end.timeIntervalSince(start) / 3600, 25, accuracy: 1e-9)
        XCTAssertEqual(CriticalPlanner.activeHours(from: start, to: end, quiet: q), 17, accuracy: 1e-9)
    }

    func testEndBeforeStartIsZero() {
        let now = at(2026, 10, 16, 10, in: vn)
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: now.addingTimeInterval(-60), quiet: quietVN), 0)
    }

    // MARK: assess

    /// Spec example: X 16, pace 50 (8% per window), 7d 55%, 25 active hours left: 55 + 40 = 95.
    func testSpecExampleIsCritical() {
        let now = at(2026, 10, 16, 8, in: vn)
        let a = CriticalPlanner.assess(usage(seven: 55, resets: now.addingTimeInterval(25 * 3600)),
                                       params: params(quiet: nil), now: now)
        // No quiet hours: high pace 80 -> 12.8% per window, 55 + 64 = 119 (not high); pace 50 -> 95.
        XCTAssertEqual(a.level, .critical)
        XCTAssertEqual(a.waste, 5, accuracy: 1e-9)
    }

    func testHighWhenEvenTheHighPaceAroundTheClockCannotFinish() {
        let now = at(2026, 10, 16, 8, in: vn)
        // 10h left (2 windows) at 80 -> 25.6; 7d 60 -> 85.6.
        let a = CriticalPlanner.assess(usage(seven: 60, resets: now.addingTimeInterval(10 * 3600)),
                                       params: params(quiet: quietVN), now: now)
        XCTAssertEqual(a.level, .high)
        XCTAssertEqual(a.waste, 14.4, accuracy: 1e-9)
    }

    func testQuietHoursCanMakeAnAccountCritical() {
        let now = at(2026, 10, 16, 10, in: vn)
        let u = usage(seven: 60, resets: now.addingTimeInterval(48 * 3600))
        XCTAssertEqual(CriticalPlanner.assess(u, params: params(quiet: nil), now: now).level, .none)  // 60 + 76.8
        let a = CriticalPlanner.assess(u, params: params(quiet: quietVN), now: now)                   // 60 + 54.4
        XCTAssertEqual(a.level, .none)
        let late = usage(seven: 50, resets: now.addingTimeInterval(48 * 3600))                       // 50 + 54.4
        XCTAssertEqual(CriticalPlanner.assess(late, params: params(quiet: quietVN), now: now).level, .none)
        let low = usage(seven: 40, resets: now.addingTimeInterval(48 * 3600))                        // 40 + 54.4
        XCTAssertEqual(CriticalPlanner.assess(low, params: params(quiet: quietVN), now: now).level, .critical)
    }

    func testExactlyOneHundredCounts() {
        let now = at(2026, 10, 16, 8, in: vn)
        // 25h, pace 50, X 16: 40 -> 7d 60 lands on exactly 100.
        let a = CriticalPlanner.assess(usage(seven: 60, resets: now.addingTimeInterval(25 * 3600)),
                                       params: params(quiet: nil), now: now)
        XCTAssertEqual(a.level, .critical)
        XCTAssertEqual(a.waste, 0, accuracy: 1e-9)
    }

    func testWeekNotStartedIsNotCritical() {
        let now = at(2026, 10, 16, 8, in: vn)
        XCTAssertEqual(CriticalPlanner.assess(usage(seven: 0, resets: nil), params: params(quiet: nil), now: now), .none)
    }

    func testMissingUsageIsNotCritical() {
        XCTAssertEqual(CriticalPlanner.assess(nil, params: params(quiet: nil), now: Date()), .none)
    }

    func testExhaustedWeekIsNotCritical() {
        let now = at(2026, 10, 16, 8, in: vn)
        XCTAssertEqual(CriticalPlanner.assess(usage(seven: 100, resets: now.addingTimeInterval(3600)),
                                              params: params(quiet: nil), now: now), .none)
        XCTAssertEqual(CriticalPlanner.assess(usage(seven: 99.5, resets: now.addingTimeInterval(3600)),
                                              params: params(quiet: nil), now: now), .none)
    }

    // MARK: badge

    func testBadgeText() {
        let p = params(quiet: quietVN)
        XCTAssertNil(CriticalPlanner.badge(for: .none, params: p))
        XCTAssertEqual(CriticalPlanner.badge(for: CriticalAssessment(level: .high, waste: 12.4), params: p),
                       CriticalBadge(level: .high, help: "High critical: at 80% of 5h per window around the clock, 7d reaches 88% by its reset (12% unused)"))
        XCTAssertEqual(CriticalPlanner.badge(for: CriticalAssessment(level: .critical, waste: 5), params: p),
                       CriticalBadge(level: .critical, help: "Critical: at 50% of 5h per window outside quiet hours (00:00 to 07:00), 7d reaches 95% by its reset (5% unused)"))
    }
}
