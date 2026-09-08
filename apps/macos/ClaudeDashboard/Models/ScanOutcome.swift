import Foundation

/// Why one scanned browser profile did not turn into an account the setup
/// sheet can offer.
///
/// The scan used to keep only two counters, so every failure collapsed into
/// one sentence that told a signed-in user to go and sign in. A profile whose
/// key had expired months ago was enough to make that sentence the summary for
/// the whole list. The outcome per profile is what the user needs, so it is
/// what the scan records.
enum ScanOutcome: Equatable {
    /// The session is valid and belongs to an account that is already stored.
    /// Dedupe is per account, not per profile, so this is the answer even when
    /// the account was added from a different profile
    /// (`contract/cases/dedupe.json`).
    case alreadyAdded(profile: String, account: String)

    /// `/api/account` rejected the session key: the profile is logged out, or
    /// the key was revoked.
    case sessionRejected(profile: String)

    /// The session is valid but belongs to no organisation with chat access,
    /// so there is no `/usage` to poll (`contract/cases/org-selection.json`).
    case noChatOrg(profile: String)

    /// The cookie would not decrypt, or the call never completed.
    case unreachable(profile: String)

    var profile: String {
        switch self {
        case let .alreadyAdded(profile, _): return profile
        case let .sessionRejected(profile): return profile
        case let .noChatOrg(profile): return profile
        case let .unreachable(profile): return profile
        }
    }

    /// The line shown for this profile, without the profile name.
    var explanation: String {
        switch self {
        case let .alreadyAdded(_, account): return "already added as \(account)"
        case .sessionRejected: return "signed out — sign in to claude.ai in this profile"
        case .noChatOrg: return "no organization with chat access"
        case .unreachable: return "could not be read"
        }
    }
}

enum ScanSummary {

    /// The message to show when the scan offers nothing, or nil when it does.
    ///
    /// `nil` for an empty scan too: "this browser has no Claude sessions at
    /// all" is the caller's sentence, because only the caller knows a profile
    /// list was even found.
    static func message(offered: Int, skipped: [ScanOutcome]) -> String? {
        guard offered == 0, !skipped.isEmpty else { return nil }

        let lines = skipped
            .sorted { $0.profile.localizedStandardCompare($1.profile) == .orderedAscending }
            .map { "\u{2022} \($0.profile): \($0.explanation)" }

        return (["Nothing new to add. What the scan found:"] + lines)
            .joined(separator: "\n")
    }
}
