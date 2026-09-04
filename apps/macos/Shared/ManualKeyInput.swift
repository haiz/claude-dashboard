import Foundation

/// Normalising a session key the user supplied by hand.
///
/// Deliberately no format check: guessing at a prefix would break the day the
/// format changes, and `/api/account` is the real validator.
enum ManualKeyInput {

    /// The key with surrounding whitespace and newlines removed, or nil when
    /// nothing is left. A trailing newline is what `pbpaste` and a here-doc both
    /// add, so trimming is not a nicety.
    static func trimmedKey(from raw: String) -> String? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
