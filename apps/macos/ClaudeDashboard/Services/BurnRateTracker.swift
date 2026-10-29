// ClaudeDashboard/Services/BurnRateTracker.swift
import Foundation

actor BurnRateTracker {
    /// Two `resetsAt` this close are the same window (as `AutoSwitchPlanner.resetTolerance`).
    static let resetTolerance: TimeInterval = 60

    let logStore: UsageLogStore

    /// Minimum seconds between two stored measurements of one key.
    static let minGap: TimeInterval = 50
    /// `A -> C` joins the rate only when it spans at most this many seconds.
    static let aWindow: TimeInterval = 300

    private struct Measurement {
        let utilization: Double
        let recordedAt: Date
        let resetsAt: Date
    }

    private struct HistoryEntry {
        /// At most three, oldest first: A, B, C.
        var samples: [Measurement]
        var streakStart: Date?
        var last: BurnRateResult?

        init(_ m: Measurement) { samples = [m] }
    }

    private var history: [String: HistoryEntry] = [:]

    init(logStore: UsageLogStore) {
        self.logStore = logStore
    }

    func record(
        accountId: UUID,
        window: UsageWindow,
        utilization: Double,
        resetsAt: Date,
        recordedAt: Date = Date()
    ) async -> BurnRateResult? {
        let key = "\(accountId.uuidString)_\(window.rawValue)"
        let isLimited = utilization >= 100.0

        // Log to store (cross-actor call requires await)
        await logStore.record(
            accountId: accountId, window: window, resetsAt: resetsAt,
            utilization: utilization, isLimited: isLimited
        )

        let m = Measurement(utilization: utilization, recordedAt: recordedAt, resetsAt: resetsAt)

        // First measurement
        guard var entry = history[key], let c = entry.samples.last else {
            history[key] = HistoryEntry(m)
            return nil
        }

        // Different reset cycle or utilization decreased: discard history and streak.
        // The API reports a boundary with jitter (sub-second, sometimes whole seconds),
        // so allow `resetTolerance`.
        guard abs(resetsAt.timeIntervalSince(c.resetsAt)) <= Self.resetTolerance,
              utilization >= c.utilization else {
            history[key] = HistoryEntry(m)
            return nil
        }

        // Minimum gap: skipped, not stored (also covers non-positive gaps).
        guard recordedAt.timeIntervalSince(c.recordedAt) >= Self.minGap else { return entry.last }

        entry.samples.append(m)
        if entry.samples.count > 3 { entry.samples.removeFirst() }
        let n = entry.samples.count
        let b = entry.samples[n - 2]
        var rate = (m.utilization - b.utilization) * 60 / m.recordedAt.timeIntervalSince(b.recordedAt)
        if n == 3 {
            let a = entry.samples[0]
            let span = m.recordedAt.timeIntervalSince(a.recordedAt)
            if span <= Self.aWindow {
                rate = max(rate, (m.utilization - a.utilization) * 60 / span)
            }
        }
        if rate >= BurnRateResult.sustainedRate {
            entry.streakStart = entry.streakStart ?? b.recordedAt
        } else {
            entry.streakStart = nil
        }
        let sustained = m.recordedAt.timeIntervalSince(entry.streakStart ?? m.recordedAt)
        entry.last = rate > 0
            ? BurnRateResult.from(ratePerMinute: rate, sustainedSeconds: sustained, utilization: m.utilization)
            : nil
        history[key] = entry
        return entry.last
    }
}
