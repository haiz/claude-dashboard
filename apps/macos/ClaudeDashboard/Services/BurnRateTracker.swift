// ClaudeDashboard/Services/BurnRateTracker.swift
import Foundation

actor BurnRateTracker {
    let logStore: UsageLogStore

    private struct Measurement {
        let utilization: Double
        let recordedAt: Date
        let resetsAt: Date
    }

    private struct HistoryEntry {
        var prev: Measurement?
        var current: Measurement?
        var lastRate: Double?  // %/second
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

        let newMeasurement = Measurement(
            utilization: utilization, recordedAt: recordedAt, resetsAt: resetsAt
        )

        guard var entry = history[key], let current = entry.current else {
            // First measurement
            history[key] = HistoryEntry(prev: nil, current: newMeasurement, lastRate: nil)
            return nil
        }

        // Different reset cycle → reset
        guard resetsAt == current.resetsAt else {
            history[key] = HistoryEntry(prev: nil, current: newMeasurement, lastRate: nil)
            return nil
        }

        // Utilization decreased → reset (anomaly or post-reset)
        guard utilization >= current.utilization else {
            history[key] = HistoryEntry(prev: nil, current: newMeasurement, lastRate: nil)
            return nil
        }

        // Utilization changed
        if utilization > current.utilization {
            let deltaPercent = utilization - current.utilization
            let deltaTime = recordedAt.timeIntervalSince(current.recordedAt)
            guard deltaTime > 0 else { return nil }

            let rate = deltaPercent / deltaTime  // %/second
            let remaining = 100.0 - utilization
            let projectedTime = remaining / rate  // seconds

            entry.prev = current
            entry.current = newMeasurement
            entry.lastRate = rate
            history[key] = entry

            return BurnRateResult.fromProjectedTime(projectedTime)
        }

        // Utilization unchanged
        let gap = recordedAt.timeIntervalSince(current.recordedAt)

        if gap >= 300 { // >= 5 minutes
            entry.current = newMeasurement
            entry.lastRate = nil
            history[key] = entry
            return nil
        }

        // Gap < 5 minutes — keep previous rate if available
        guard let lastRate = entry.lastRate, entry.prev != nil else {
            entry.current = newMeasurement
            history[key] = entry
            return nil
        }

        let remaining = 100.0 - utilization
        guard remaining > 0 else {
            return BurnRateResult.fromProjectedTime(0)
        }
        let projectedTime = remaining / lastRate
        entry.current = newMeasurement
        history[key] = entry
        return BurnRateResult.fromProjectedTime(projectedTime)
    }
}
