// ClaudeDashboard/Models/UsageLogModels.swift
import Foundation

enum UsageWindow: Int, CaseIterable {
    case fiveHour = 0
    case sevenDay = 1
    case sonnet = 2

    var label: String {
        switch self {
        case .fiveHour: return "5h"
        case .sevenDay: return "7d"
        case .sonnet: return "S"
        }
    }
}

struct UsageLogEntry: Identifiable, Equatable {
    let id: Int64
    let accountId: UUID
    let window: UsageWindow
    let resetsAt: Date
    let recordedAt: Date
    let utilization: Double    // 0-100
    let isLimited: Bool
}

struct ResetCycle: Identifiable, Equatable {
    var id: Date { resetsAt }
    let resetsAt: Date
    let firstRecordedAt: Date
    let lastRecordedAt: Date
    let peakUtilization: Double
    let dataPointCount: Int
}

struct BurnRateResult: Equatable {
    let level: Int              // 1-5
    let animal: String          // emoji
    let projectedTime: TimeInterval  // seconds until 100%

    static let animals: [Int: String] = [
        1: "🐌",  // > 5h
        2: "🐢",  // 3-5h
        3: "🐇",  // 1.5-3h
        4: "🐎",  // 0.5-1.5h
        5: "🐆",  // < 30m
    ]

    static func fromProjectedTime(_ seconds: TimeInterval) -> BurnRateResult {
        let hours = seconds / 3600
        let level: Int
        if hours > 5 { level = 1 }
        else if hours > 3 { level = 2 }
        else if hours > 1.5 { level = 3 }
        else if hours > 0.5 { level = 4 }
        else { level = 5 }
        return BurnRateResult(level: level, animal: animals[level]!, projectedTime: seconds)
    }
}

struct BurnRates: Equatable {
    var fiveHour: BurnRateResult?
    var sevenDay: BurnRateResult?
    var sonnet: BurnRateResult?
}

// MARK: - Reset Transition Injection

extension Array where Element == UsageLogEntry {
    /// Injects synthetic data points at reset boundaries so chart lines show
    /// accurate step-down transitions instead of diagonal slopes.
    ///
    /// For each consecutive pair of logs (same account) where `resetsAt` changes,
    /// inserts up to two points:
    /// 1. A "hold" point at `prev.resetsAt - 1s` with the previous utilization
    /// 2. A "drop" point at `prev.resetsAt` with 0% utilization
    func withResetTransitions() -> [UsageLogEntry] {
        guard count >= 2 else { return self }

        var syntheticId: Int64 = -1
        var result: [UsageLogEntry] = []

        let grouped = Dictionary(grouping: self) { $0.accountId }

        for (_, accountLogs) in grouped {
            let sorted = accountLogs.sorted { $0.recordedAt < $1.recordedAt }

            for i in 0..<sorted.count {
                let current = sorted[i]

                if i > 0 {
                    let prev = sorted[i - 1]

                    if prev.resetsAt != current.resetsAt
                        && prev.resetsAt > prev.recordedAt
                        && prev.resetsAt <= current.recordedAt
                    {
                        let holdTime = prev.resetsAt.addingTimeInterval(-1)

                        // Point 1: hold at old utilization (skip if too close to prev log)
                        if holdTime > prev.recordedAt {
                            result.append(UsageLogEntry(
                                id: syntheticId,
                                accountId: current.accountId,
                                window: current.window,
                                resetsAt: prev.resetsAt,
                                recordedAt: holdTime,
                                utilization: prev.utilization,
                                isLimited: prev.isLimited
                            ))
                            syntheticId -= 1
                        }

                        // Point 2: drop to 0%
                        result.append(UsageLogEntry(
                            id: syntheticId,
                            accountId: current.accountId,
                            window: current.window,
                            resetsAt: current.resetsAt,
                            recordedAt: prev.resetsAt,
                            utilization: 0.0,
                            isLimited: false
                        ))
                        syntheticId -= 1
                    }
                }

                result.append(current)
            }
        }

        result.sort { $0.recordedAt < $1.recordedAt }
        return result
    }
}
