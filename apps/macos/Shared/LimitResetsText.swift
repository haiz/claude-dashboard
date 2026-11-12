import Foundation

/// Display copy for limit resets: the account page's detail line, the gauge
/// badge's tooltip and the relative "in N days" text. Pure: time, calendar
/// (its time zone is the local time shown) and locale are passed in.
///
/// Rules: `contract/README.md` "Limit resets". The year is shown only when the
/// expiry is not in `now`'s year.
///
///     let line = LimitResetsText.detailLine(kind: .full, summary: s, now: now,
///                                           calendar: .current, locale: .current)
///     // ("Full reset · 1", "Expires Dec 10, 22:00 (in 28 days)")
///     LimitResetsText.tooltip(kind: .fable, summary: s2, now: now, calendar: .current, locale: .current)
///     // "Fable resets · 2 · expire Mar 9, 2027, 14:59"
enum LimitResetsText {
    static func detailLine(kind: ResetKind, summary: ResetSummary, now: Date,
                           calendar: Calendar, locale: Locale) -> (title: String, expiry: String) {
        let when = expiryDate(summary.expiresAt, now: now, calendar: calendar, locale: locale)
        let left = relative(summary.expiresAt.timeIntervalSince(now))
        return ("\(name(kind)) reset · \(summary.count)", "Expires \(when) (\(left))")
    }

    /// One reset: "Full reset · expires ...". Several: "Fable resets · 2 · expire ...".
    static func tooltip(kind: ResetKind, summary: ResetSummary, now: Date,
                        calendar: Calendar, locale: Locale) -> String {
        let when = expiryDate(summary.expiresAt, now: now, calendar: calendar, locale: locale)
        if summary.count == 1 {
            return "\(name(kind)) reset · expires \(when)"
        }
        return "\(name(kind)) resets · \(summary.count) · expire \(when)"
    }

    /// `>= 1 day`: "in N days" ("in 1 day"); under a day: "in N h", N at least 1.
    static func relative(_ seconds: TimeInterval) -> String {
        if seconds >= 86_400 {
            let days = Int(seconds / 86_400)
            return days == 1 ? "in 1 day" : "in \(days) days"
        }
        return "in \(max(1, Int(seconds / 3_600))) h"
    }

    // MARK: - Private

    private static func name(_ kind: ResetKind) -> String {
        switch kind {
        case .full: return "Full"
        case .fable: return "Fable"
        }
    }

    private static func expiryDate(_ date: Date, now: Date, calendar: Calendar, locale: Locale) -> String {
        let formatter = DateFormatter()
        formatter.calendar = calendar
        formatter.timeZone = calendar.timeZone
        formatter.locale = locale
        let sameYear = calendar.component(.year, from: date) == calendar.component(.year, from: now)
        formatter.dateFormat = sameYear ? "MMM d, HH:mm" : "MMM d, yyyy, HH:mm"
        return formatter.string(from: date)
    }
}
