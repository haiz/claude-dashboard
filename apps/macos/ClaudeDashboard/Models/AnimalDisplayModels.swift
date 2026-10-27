import Foundation

/// Shared rules for the optional animated 5-hour burn-rate display.
enum AnimalDisplayModels {
    static let cycleSeconds: TimeInterval = 10
    static let activitySeconds: TimeInterval = 600
    static let resetToleranceSeconds: TimeInterval = 60
    static let frameCount = 24
    static let frameRate: Double = 12
    static let showBurnAnimalsKey = "showBurnAnimals"
    static let defaultShowBurnAnimals = false

    enum Content: String, Equatable {
        case percentage
        case percentageToAnimal
        case animal
        case animalToPercentage
    }

    struct Phase {
        let phase: Double
        let content: Content
        let transitionProgress: Double?
        let percentageOpacity: Double
        let animalOpacity: Double
        let frameIndex: Int?

        /// True when a gauge draws the same from both phases: same content, frame and layer
        /// opacities. Any two percentage-only phases render the same whatever their time.
        func rendersSame(as other: Phase) -> Bool {
            content == other.content && frameIndex == other.frameIndex
                && percentageOpacity == other.percentageOpacity && animalOpacity == other.animalOpacity
        }

        static func at(elapsedSeconds: Double) -> Phase {
            let phase = elapsedSeconds.truncatingRemainder(dividingBy: cycleSeconds)
            let normalized = phase < 0 ? phase + cycleSeconds : phase
            if normalized < 5.5 {
                return Phase(phase: normalized, content: .percentage, transitionProgress: nil,
                             percentageOpacity: 1, animalOpacity: 0, frameIndex: nil)
            }
            if normalized < 6 {
                let progress = (normalized - 5.5) / 0.5
                return Phase(phase: normalized, content: .percentageToAnimal,
                             transitionProgress: progress, percentageOpacity: 1 - progress,
                             animalOpacity: progress, frameIndex: 0)
            }
            let frame = Int(floor((normalized - 6) * frameRate)) % frameCount
            if normalized < 9.5 {
                return Phase(phase: normalized, content: .animal, transitionProgress: nil,
                             percentageOpacity: 0, animalOpacity: 1, frameIndex: frame)
            }
            let progress = (normalized - 9.5) / 0.5
            return Phase(phase: normalized, content: .animalToPercentage,
                         transitionProgress: progress, percentageOpacity: progress,
                         animalOpacity: 1 - progress, frameIndex: frame)
        }
    }

    /// Every stable asset id `assetID(for:)` can return, in burn-rate order.
    static let assetIDs = ["snail", "turtle", "rabbit", "horse", "cheetah"]

    /// Asset directory names are stable contract identifiers, independent of localized emoji.
    static func assetID(for emoji: String) -> String? {
        switch emoji {
        case "🐌": return "snail"
        case "🐢": return "turtle"
        case "🐇": return "rabbit"
        case "🐎": return "horse"
        case "🐆": return "cheetah"
        default: return nil
        }
    }

    static func eligibleAsset(showBurnAnimals: Bool, isFiveHour: Bool,
                              emoji: String?, isActive: Bool) -> String? {
        guard showBurnAnimals, isFiveHour, isActive, let emoji else { return nil }
        return assetID(for: emoji)
    }
}

/// Tracks recent 5-hour changes by account identity. Failed refreshes are represented by
/// not calling `recordSuccess`, so they cannot extend the activity window.
struct AnimalActivityTracker {
    private struct Observation {
        let utilization: Double
        let resetsAt: TimeInterval?
    }

    private var previous: [String: Observation] = [:]
    private var lastChange: [String: TimeInterval] = [:]

    mutating func recordSuccess(accountID: String, utilization: Double,
                                resetsAt: TimeInterval?, at: TimeInterval) {
        let next = Observation(utilization: utilization, resetsAt: resetsAt)
        if let old = previous[accountID] {
            let sameReset: Bool
            switch (old.resetsAt, resetsAt) {
            case let (a?, b?): sameReset = abs(a - b) <= AnimalDisplayModels.resetToleranceSeconds
            case (nil, nil): sameReset = true
            default: sameReset = false
            }
            if old.utilization != utilization || !sameReset {
                lastChange[accountID] = at
            }
        }
        previous[accountID] = next
    }

    func isActive(accountID: String, at: TimeInterval) -> Bool {
        guard let changed = lastChange[accountID] else { return false }
        return at - changed <= AnimalDisplayModels.activitySeconds
    }

    func lastChangeAt(accountID: String) -> TimeInterval? { lastChange[accountID] }
}
