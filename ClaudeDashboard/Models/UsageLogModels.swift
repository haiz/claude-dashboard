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
