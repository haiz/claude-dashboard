import Foundation

/// The `claudeAiOauth` object from Claude Code's Keychain entry, carried verbatim.
///
/// Only the fields the switcher decides on are parsed; `json` keeps every field, so a
/// write never drops one a newer Claude Code added.
///
///     let c = OAuthCredential(json: data)   // nil unless `data` is a JSON object
///     c?.isBlank                             // true once Claude Code wiped it
struct OAuthCredential: Equatable {
    /// The object serialized with sorted keys, so equal credentials compare equal.
    let json: Data
    let refreshToken: String
    let expiresAt: Date?
    let refreshTokenExpiresAt: Date?

    init?(json: Data) {
        guard let object = (try? JSONSerialization.jsonObject(with: json)) as? [String: Any] else {
            return nil
        }
        self.init(object: object)
    }

    init?(object: [String: Any]) {
        guard let canonical = OAuthAccountJSON.canonical(object) else { return nil }
        self.json = canonical
        self.refreshToken = object["refreshToken"] as? String ?? ""
        self.expiresAt = Self.date(object["expiresAt"])
        self.refreshTokenExpiresAt = Self.date(object["refreshTokenExpiresAt"])
    }

    /// The object form, for embedding in a larger JSON document.
    var object: [String: Any] { OAuthAccountJSON.object(json) ?? [:] }

    /// Claude Code blanks the entry after a failed refresh: empty tokens, `expiresAt` 0.
    var isBlank: Bool { refreshToken.isEmpty }

    /// Claude Code writes epoch milliseconds; 0 and absent both mean "none".
    private static func date(_ value: Any?) -> Date? {
        guard let ms = (value as? NSNumber)?.doubleValue, ms > 0 else { return nil }
        return Date(timeIntervalSince1970: ms / 1000)
    }
}

/// What the app keeps per account: the credential, plus the `oauthAccount` object that
/// `~/.claude.json` must carry while that credential is active. Both verbatim.
struct VaultEntry: Equatable {
    let oauth: OAuthCredential
    /// Canonical JSON (see `OAuthAccountJSON.canonical`).
    let oauthAccount: Data
}

/// Helpers for JSON objects kept as canonical `Data`.
enum OAuthAccountJSON {
    /// Sorted-key, unescaped-slash serialization; nil unless `object` is a dictionary.
    static func canonical(_ object: Any) -> Data? {
        guard object is [String: Any] else { return nil }
        return try? JSONSerialization.data(
            withJSONObject: object, options: [.sortedKeys, .withoutEscapingSlashes])
    }

    static func object(_ json: Data) -> [String: Any]? {
        (try? JSONSerialization.jsonObject(with: json)) as? [String: Any]
    }

    /// The `emailAddress` of an `oauthAccount` object.
    static func email(_ json: Data) -> String? {
        object(json)?["emailAddress"] as? String
    }
}
