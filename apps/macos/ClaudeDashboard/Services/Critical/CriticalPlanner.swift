import Foundation

/// Daily hours not counted as usable time, as whole hours in `timeZone`. A start after the
/// end crosses midnight (22 to 6).
struct QuietHours: Equatable {
    let startHour: Int
    let endHour: Int
    let timeZone: TimeZone
}

enum CriticalLevel: Equatable {
    case none, critical, high

    var label: String {
        switch self {
        case .none: return "not critical"
        case .critical: return "critical"
        case .high: return "high critical"
        }
    }
}

/// `waste`: the 7d percent still unused at the reset under the level's own pace.
struct CriticalAssessment: Equatable {
    let level: CriticalLevel
    let waste: Double
    static let none = CriticalAssessment(level: .none, waste: 0)
}

struct CriticalParameters: Equatable {
    let x: Double
    let pace: Double
    let highPace: Double
    let quiet: QuietHours?
}

struct CriticalBadge: Equatable {
    let level: CriticalLevel
    let help: String
}

/// Whether an account's 7d quota will go unused. Pure; spec
/// `docs/superpowers/specs/2026-10-16-critical-accounts-design.md`, "Assessment".
///
///     let a = CriticalPlanner.assess(usage, params: settings.parameters(x: 16.3), now: Date())
///     a.level   // .critical
enum CriticalPlanner {
    static let windowHours = 5.0
    /// A week at or above this has nothing meaningful left; matches auto switch's 7d tier limit.
    static let sevenDayLimit = 99.0

    /// Hours in `[from, to)` outside the quiet hours.
    static func activeHours(from start: Date, to end: Date, quiet: QuietHours?) -> Double {
        guard end > start else { return 0 }
        var seconds = end.timeIntervalSince(start)
        guard let quiet, quiet.startHour != quiet.endHour else { return seconds / 3600 }
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = quiet.timeZone
        // Start a day early: a block that began yesterday evening may still run.
        var day = cal.date(byAdding: .day, value: -1, to: cal.startOfDay(for: start))!
        while day < end {
            let blockStart = cal.date(bySettingHour: quiet.startHour, minute: 0, second: 0, of: day)!
            let endDay = quiet.endHour > quiet.startHour ? day : cal.date(byAdding: .day, value: 1, to: day)!
            let blockEnd = cal.date(bySettingHour: quiet.endHour, minute: 0, second: 0, of: endDay)!
            let overlap = min(blockEnd, end).timeIntervalSince(max(blockStart, start))
            if overlap > 0 { seconds -= overlap }
            day = cal.date(byAdding: .day, value: 1, to: day)!
        }
        return max(seconds, 0) / 3600
    }

    /// 7d at the reset if each 5h window from now uses `pace`% of its limit.
    static func projected(sevenDay: Double, activeHours: Double, pace: Double, x: Double) -> Double {
        sevenDay + activeHours / windowHours * pace / 100 * x
    }

    static func assess(_ usage: UsageData?, params: CriticalParameters, now: Date) -> CriticalAssessment {
        guard let usage, let resetsAt = usage.sevenDay.resetsAt, resetsAt > now, params.x > 0,
              usage.sevenDay.utilization < sevenDayLimit else { return .none }
        let seven = usage.sevenDay.utilization
        let high = projected(sevenDay: seven, activeHours: activeHours(from: now, to: resetsAt, quiet: nil),
                             pace: params.highPace, x: params.x)
        if high <= 100 { return CriticalAssessment(level: .high, waste: 100 - high) }
        let normal = projected(sevenDay: seven, activeHours: activeHours(from: now, to: resetsAt, quiet: params.quiet),
                               pace: params.pace, x: params.x)
        if normal <= 100 { return CriticalAssessment(level: .critical, waste: 100 - normal) }
        return .none
    }

    static func badge(for assessment: CriticalAssessment, params: CriticalParameters) -> CriticalBadge? {
        let reach = Int((100 - assessment.waste).rounded())
        let unused = Int(assessment.waste.rounded())
        switch assessment.level {
        case .none:
            return nil
        case .high:
            return CriticalBadge(level: .high, help: "High critical: at \(Int(params.highPace))% of 5h per window around the clock, 7d reaches \(reach)% by its reset (\(unused)% unused)")
        case .critical:
            let quiet = params.quiet.map { String(format: " outside quiet hours (%02d:00 to %02d:00)", $0.startHour, $0.endHour) } ?? " around the clock"
            return CriticalBadge(level: .critical, help: "Critical: at \(Int(params.pace))% of 5h per window\(quiet), 7d reaches \(reach)% by its reset (\(unused)% unused)")
        }
    }
}
