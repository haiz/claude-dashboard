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
