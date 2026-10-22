import Foundation

/// How an auto switch evaluation ended. Raw values are stored in `auto_switch_logs.outcome`
/// and are never renumbered.
enum AutoSwitchOutcome: Int, CaseIterable {
    case switched = 0
    case failed = 1
    case noCandidate = 2   // logged once per stretch, not on every refresh

    var label: String {
        switch self {
        case .switched: return "Switched"
        case .failed: return "Failed"
        case .noCandidate: return "No candidate"
        }
    }
}

/// Who made a switch: auto switch, or the user's Switch button. Raw values are stored in
/// `auto_switch_logs.trig` and are never renumbered; rows logged before the column existed
/// read as `.auto`.
enum AutoSwitchTrigger: Int, CaseIterable {
    case auto = 0
    case manual = 1

    var label: String {
        switch self {
        case .auto: return "Auto"
        case .manual: return "Manual"
        }
    }
}

/// One row of the Switch Log. Account names are copied in when logged, so a row still
/// reads right after the account is renamed or removed.
///
///     AutoSwitchLogEntry(id: 1, date: now, trigger: .auto, outcome: .switched, fromName: "a@x.com",
///                        toName: "b@x.com", reason: "5h 96% >= 95%, tier 1",
///                        detail: "b@x.com: 5h 12%, 7d 30%")
struct AutoSwitchLogEntry: Identifiable, Equatable {
    let id: Int64
    let date: Date
    let trigger: AutoSwitchTrigger
    let outcome: AutoSwitchOutcome
    let fromName: String?
    let toName: String?
    /// Why auto switch acted: the window over its limit and the tier, or the 5h reset;
    /// "Manual switch" for the Switch button.
    let reason: String
    /// The target's usage, or the error for a failed switch.
    let detail: String?
}
