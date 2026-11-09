import Foundation

/// What the account chart's range label shows: the start and end of the visible range
/// (the end reads "now" within 60 s of now) and, when the range holds one unbroken
/// stretch of the window, the utilization at each edge.
/// Rules: `contract/cases/chart-window-summary.json`; Rust twin `core::chart::window_summary`.
///
///     // 5h chart, last 5 h, 2% -> 9%, no reset:
///     ChartWindowSummary.make(logs: logs, range: range, now: now, window: .fiveHour)
///     // endIsNow true, startPct 2, endPct 9 -> time "14:02 → now", usage "2% → 9%"
struct ChartWindowSummary: Equatable {
    let endIsNow: Bool
    /// Rounded utilization at the range edges; both set or both nil.
    let startPct: Int?
    let endPct: Int?

    static let nowTolerance: TimeInterval = 60

    /// Longest range that still shows percentages, or nil for a window that never does.
    static func percentLimit(for window: UsageWindow) -> TimeInterval? {
        switch window {
        case .fiveHour: return 5 * 3600
        case .sevenDay: return 7 * 86400
        case .fable: return nil
        }
    }

    /// `logs` ascending, through `withResetTransitions`, with context points outside `range`.
    static func make(logs: [UsageLogEntry], range: ClosedRange<Date>, now: Date,
                     window: UsageWindow) -> ChartWindowSummary {
        let endIsNow = abs(now.timeIntervalSince(range.upperBound)) <= nowTolerance
        let hidden = ChartWindowSummary(endIsNow: endIsNow, startPct: nil, endPct: nil)
        guard let limit = percentLimit(for: window),
              range.upperBound.timeIntervalSince(range.lowerBound) <= limit else { return hidden }
        let hasReset = zip(logs, logs.dropFirst()).contains { prev, cur in
            cur.recordedAt > range.lowerBound && cur.recordedAt <= range.upperBound
                && prev.utilization > 0 && cur.utilization == 0
        }
        guard !hasReset,
              let start = value(at: range.lowerBound, in: logs),
              let end = value(at: range.upperBound, in: logs) else { return hidden }
        let (s, e) = (Int(start.rounded()), Int(end.rounded()))
        guard s != e else { return hidden }
        return ChartWindowSummary(endIsNow: endIsNow, startPct: s, endPct: e)
    }

    /// nil with no log at or before `date`; else linear between the logs around it, or the
    /// last value past the final log.
    static func value(at date: Date, in logs: [UsageLogEntry]) -> Double? {
        guard let b = logs.lastIndex(where: { $0.recordedAt <= date }) else { return nil }
        let before = logs[b]
        guard b + 1 < logs.count, logs[b + 1].recordedAt > before.recordedAt else { return before.utilization }
        let after = logs[b + 1]
        let f = date.timeIntervalSince(before.recordedAt) / after.recordedAt.timeIntervalSince(before.recordedAt)
        return before.utilization + (after.utilization - before.utilization) * f
    }

    /// The label's two parts, drawn in different styles: `time` ("14:02 → now",
    /// "7 Nov 09:00 → 7 Nov 14:00") and `usage` ("2% → 9%", nil when the percentages are
    /// hidden). A date the calendar puts on another day than `now` gets its day and month.
    func label(range: ClosedRange<Date>, now: Date, calendar: Calendar = .current) -> (time: String, usage: String?) {
        func stamp(_ date: Date) -> String {
            let df = DateFormatter()
            df.locale = Locale(identifier: "en_US_POSIX")
            df.calendar = calendar
            df.timeZone = calendar.timeZone
            df.dateFormat = calendar.isDate(date, inSameDayAs: now) ? "HH:mm" : "d MMM HH:mm"
            return df.string(from: date)
        }
        let time = "\(stamp(range.lowerBound)) → \(endIsNow ? "now" : stamp(range.upperBound))"
        guard let s = startPct, let e = endPct else { return (time, nil) }
        return (time, "\(s)% → \(e)%")
    }
}
