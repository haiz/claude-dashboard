import XCTest
@testable import ClaudeDashboard

/// Copy for the limit reset badges and the account page's "Limit resets" section.
final class LimitResetsTextTests: XCTestCase {
    private var calendar: Calendar {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
        return cal
    }
    private let locale = Locale(identifier: "en_US")
    private let now = date("2026-11-12T00:00:00Z")

    private let full = ResetSummary(count: 1, expiresAt: date("2026-12-10T22:00:00Z"), urgent: false)
    private let fable = ResetSummary(count: 2, expiresAt: date("2027-03-09T07:59:00Z"), urgent: false)

    func testFullDetailLine() {
        let line = LimitResetsText.detailLine(kind: .full, summary: full, now: now, calendar: calendar, locale: locale)
        XCTAssertEqual(line.title, "Full reset · 1")
        XCTAssertEqual(line.expiry, "Expires Dec 11, 05:00 (in 28 days)")
    }

    func testFableDetailLineShowsYearWhenNotCurrent() {
        let line = LimitResetsText.detailLine(kind: .fable, summary: fable, now: now, calendar: calendar, locale: locale)
        XCTAssertEqual(line.title, "Fable reset · 2")
        XCTAssertEqual(line.expiry, "Expires Mar 9, 2027, 14:59 (in 117 days)")
    }

    func testRelative() {
        XCTAssertEqual(LimitResetsText.relative(86_400), "in 1 day")
        XCTAssertEqual(LimitResetsText.relative(86_399), "in 23 h")
        XCTAssertEqual(LimitResetsText.relative(120), "in 1 h")
        XCTAssertEqual(LimitResetsText.relative(28 * 86_400 + 79_200), "in 28 days")
        XCTAssertEqual(LimitResetsText.relative(5 * 3_600 + 59), "in 5 h")
    }

    func testTooltipPluralForSeveral() {
        XCTAssertEqual(LimitResetsText.tooltip(kind: .fable, summary: fable, now: now, calendar: calendar, locale: locale),
                       "Fable resets · 2 · expire Mar 9, 2027, 14:59")
    }

    func testTooltipSingularForOne() {
        XCTAssertEqual(LimitResetsText.tooltip(kind: .full, summary: full, now: now, calendar: calendar, locale: locale),
                       "Full reset · expires Dec 11, 05:00")
    }
}

private func date(_ text: String) -> Date {
    ISO8601DateFormatter().date(from: text)!
}
