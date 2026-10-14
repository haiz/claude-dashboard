import Foundation

/// What started a ping. Raw values are stored in `ping_logs.trig` and are never renumbered.
enum PingTrigger: Int, CaseIterable {
    case manual = 0   // the Ping button on an account page
    case auto = 1     // Auto Ping, after a refresh found a window not running

    var label: String {
        switch self {
        case .manual: return "Manual"
        case .auto: return "Auto"
        }
    }
}

/// One finished ping, as the Ping Log stores it.
struct PingLogEntry: Identifiable, Equatable {
    let id: Int64
    let accountId: UUID
    let trigger: PingTrigger
    let startedAt: Date
    let finishedAt: Date
    let outcome: PingOutcome
    let detail: String?

    /// The line under an account page's Ping button.
    ///
    ///     entry.caption(time: { $0.formatted(date: .omitted, time: .shortened) })
    ///     // "Pinged at 10:44" or "Ping failed at 10:44: Session expired"
    func caption(time: (Date) -> String) -> String {
        let at = time(finishedAt)
        if outcome == .ok {
            return detail.map { "Pinged at \(at) (\($0))" } ?? "Pinged at \(at)"
        }
        let reason = detail.map { "\(outcome.label), \($0)" } ?? outcome.label
        return "Ping failed at \(at): \(reason)"
    }
}
