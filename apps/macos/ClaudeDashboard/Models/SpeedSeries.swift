// ClaudeDashboard/Models/SpeedSeries.swift
import Foundation

/// Plan weights: how many times a Pro account's usage a plan allows. Shared by the Overview
/// chart's Avg overlay and the Speed chart. Rules in `contract/README.md`, "Speed series".
enum PlanWeight {
    static func weight(_ plan: AccountPlan) -> Double {
        switch plan {
        case .pro: return 1
        case .max5x: return 5
        case .max20x: return 20
        case .max200: return 10
        // Usage multiplier not verified; neutral, like Windows' fallback.
        case .team, .enterprise, .free: return 1
        }
    }

    /// Scale that turns an account's speed into Max-equivalent %/h: `weight / 10`.
    static func maxFactor(_ plan: AccountPlan) -> Double { weight(plan) / 10 }
}

/// Bucket width of the Overview Speed chart.
enum SpeedStep: Int, CaseIterable, Identifiable {
    case six = 6, ten = 10, twenty = 20, thirty = 30

    var id: Int { rawValue }
    var seconds: TimeInterval { TimeInterval(rawValue * 60) }
    var label: String { "\(rawValue)m" }
}

/// One bucket's speed in Max-equivalent %/h, stamped at the bucket end.
struct SpeedPoint: Equatable {
    let end: Date
    let value: Double
}

/// Per-bucket speed series for the Overview Speed chart. Same rules as the Rust
/// `core::speed_series`; cases in `contract/cases/speed-series.json`.
///
///     // One 1% rise inside the 00:00-00:06 bucket reads 10 %/h for a Max account.
///     SpeedSeries.account(entries: entries, step: 360, utcOffset: 0,
///                         range: from...to, factor: PlanWeight.maxFactor(.max200))
enum SpeedSeries {
    private static let hour: TimeInterval = 3600
    /// An account whose newest entry is at most this old is "live" for `total`.
    private static let liveSeconds: TimeInterval = 900

    /// Per-bucket speed of one account, scaled by `factor`.
    ///
    /// `entries` belong to one account, ascending by `recordedAt`, already through the
    /// reset-transition pass. Buckets are `step` wide, aligned to the local clock
    /// (`utcOffset`), and are the ones overlapping `range`. A bucket gets a point only when
    /// the entries cover it whole (first entry at or before its start, last at or after its end).
    static func account(entries: [UsageLogEntry], step: TimeInterval, utcOffset: TimeInterval,
                        range: ClosedRange<Date>, factor: Double) -> [SpeedPoint] {
        guard let first = entries.first, let last = entries.last else { return [] }
        let from = range.lowerBound.timeIntervalSince1970
        let to = range.upperBound.timeIntervalSince1970
        func index(_ t: Double) -> Int { Int(((t + utcOffset) / step).rounded(.down)) }
        func start(_ k: Int) -> Double { Double(k) * step - utcOffset }

        let kFirst = index(from)
        let kLast = Int(((to + utcOffset) / step).rounded(.up)) - 1
        guard kLast >= kFirst else { return [] }
        var sums = [Double](repeating: 0, count: kLast - kFirst + 1)

        for (a, b) in zip(entries, entries.dropFirst()) {
            let rise = b.utilization - a.utilization
            guard rise > 0 else { continue }
            let ta = a.recordedAt.timeIntervalSince1970
            let tb = b.recordedAt.timeIntervalSince1970
            if tb == ta {
                let k = index(tb)
                if k >= kFirst && k <= kLast { sums[k - kFirst] += rise }
                continue
            }
            let span = tb - ta
            let lo = max(index(ta), kFirst), hi = min(index(tb), kLast)
            guard lo <= hi else { continue }
            for k in lo...hi {
                let overlap = max(min(start(k + 1), tb) - max(start(k), ta), 0)
                sums[k - kFirst] += rise * overlap / span
            }
        }

        let firstT = first.recordedAt.timeIntervalSince1970
        let lastT = last.recordedAt.timeIntervalSince1970
        return (kFirst...kLast)
            .filter { firstT <= start($0) && lastT >= start($0 + 1) }
            .map { SpeedPoint(end: Date(timeIntervalSince1970: start($0 + 1)),
                              value: sums[$0 - kFirst] / (step / hour) * factor) }
    }

    /// One account's already scaled points and the time of its newest entry.
    struct AccountInput {
        let points: [SpeedPoint]
        let lastEntry: Date
    }

    /// Total speed: per bucket end, the sum over the accounts that have a point there.
    ///
    /// An account is live when `now - lastEntry <= 15 min`. While any live account has points,
    /// the Total stops at the earliest last point end among them, so it never draws a bucket a
    /// live account has not reached. Stale accounts do not hold it back.
    static func total(_ accounts: [AccountInput], now: Date) -> [SpeedPoint] {
        var sums: [Date: Double] = [:]
        for account in accounts {
            for p in account.points { sums[p.end, default: 0] += p.value }
        }
        let cutoff = accounts
            .filter { now.timeIntervalSince($0.lastEntry) <= liveSeconds }
            .compactMap { $0.points.last?.end }
            .min()
        return sums
            .filter { cutoff == nil || $0.key <= cutoff! }
            .map { SpeedPoint(end: $0.key, value: $0.value) }
            .sorted { $0.end < $1.end }
    }
}
