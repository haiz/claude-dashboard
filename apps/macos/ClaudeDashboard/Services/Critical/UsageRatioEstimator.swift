import Foundation

/// X: the share of the 7d quota (in %) that one full 5h window costs, for all accounts
/// together. Spec: `docs/superpowers/specs/2026-10-16-critical-accounts-design.md`, "Estimate".
///
///     let e = UsageRatioEstimator.estimate(fiveHour: fiveRows, sevenDay: sevenRows)
///     e?.x   // 16.3, or nil when the logs hold under 100 points of 5h use
struct UsageRatioEstimate: Equatable {
    let x: Double
    let sumFiveHour: Double
    let sumSevenDay: Double
}

enum UsageRatioEstimator {
    static let lookback: TimeInterval = 3 * 86_400
    /// How far back 7d rows are read beyond `lookback`: compression can leave the last
    /// row of an unchanged 7d value days before the 5h window that needs it.
    static let sevenDayLookback: TimeInterval = 7 * 86_400
    static let minWindowDelta = 10.0
    static let minTotalFiveHour = 100.0
    static let defaultX = 16.0
    /// `resetsAt` drifts by a second or more between fetches; a real reset moves it by hours.
    static let resetTolerance: TimeInterval = 60
    /// The 5h and 7d rows of one refresh are written separately and can land a second apart.
    static let sampleTolerance: TimeInterval = 5

    static func estimate(fiveHour: [UsageLogEntry], sevenDay: [UsageLogEntry]) -> UsageRatioEstimate? {
        let weekly = Dictionary(grouping: sevenDay, by: \.accountId)
            .mapValues { $0.sorted { $0.recordedAt < $1.recordedAt } }
        var sumFive = 0.0
        var sumSeven = 0.0
        for window in windows(fiveHour) {
            guard let first = window.first, let last = window.last,
                  last.utilization - first.utilization >= minWindowDelta,
                  let series = weekly[first.accountId],
                  let w0 = reading(series, at: first.recordedAt),
                  let w1 = reading(series, at: last.recordedAt),
                  abs(w1.resetsAt.timeIntervalSince(w0.resetsAt)) <= resetTolerance,
                  // 7d fell without a week reset: a correction, not a cost; it would pull X down.
                  w1.utilization - w0.utilization >= 0 else { continue }
            sumFive += last.utilization - first.utilization
            sumSeven += w1.utilization - w0.utilization
        }
        guard sumFive >= minTotalFiveHour else { return nil }
        return UsageRatioEstimate(x: 100 * sumSeven / sumFive, sumFiveHour: sumFive, sumSevenDay: sumSeven)
    }

    /// Per account, in time order; a `resetsAt` more than `resetTolerance` away from the
    /// current window's starts the next window.
    private static func windows(_ rows: [UsageLogEntry]) -> [[UsageLogEntry]] {
        var result: [[UsageLogEntry]] = []
        for (_, accountRows) in Dictionary(grouping: rows, by: \.accountId) {
            var current: [UsageLogEntry] = []
            for row in accountRows.sorted(by: { $0.recordedAt < $1.recordedAt }) {
                if let anchor = current.first,
                   abs(row.resetsAt.timeIntervalSince(anchor.resetsAt)) > resetTolerance {
                    result.append(current)
                    current = []
                }
                current.append(row)
            }
            if !current.isEmpty { result.append(current) }
        }
        return result
    }

    private static func reading(_ series: [UsageLogEntry], at time: Date) -> UsageLogEntry? {
        series.last { $0.recordedAt <= time.addingTimeInterval(sampleTolerance) }
    }
}
