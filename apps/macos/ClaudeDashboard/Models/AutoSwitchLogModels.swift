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

/// One row of the Switch Log. Account names are copied in when logged, so a row still
/// reads right after the account is renamed or removed.
///
///     AutoSwitchLogEntry(id: 1, date: now, outcome: .switched, fromName: "a@x.com",
///                        toName: "b@x.com", reason: "5h 96% >= 95%, tier 1",
///                        detail: "b@x.com: 5h 12%, 7d 30%")
struct AutoSwitchLogEntry: Identifiable, Equatable {
    let id: Int64
    let date: Date
    let outcome: AutoSwitchOutcome
    let fromName: String?
    let toName: String?
    /// Why auto switch acted: the window over its limit and the tier, or the 5h reset.
    let reason: String
    /// The target's usage, or the error for a failed switch.
    let detail: String?
}
