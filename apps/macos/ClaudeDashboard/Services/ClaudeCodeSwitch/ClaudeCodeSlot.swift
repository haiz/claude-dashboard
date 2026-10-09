import Foundation

/// Failure reading or writing Claude Code's Keychain entry.
enum ClaudeCodeSlotError: Error, Equatable {
    /// The entry exists but is not a JSON object; nothing was written.
    case unreadableEntry
    /// The entry's `claudeAiOauth` is no longer what the caller read (a Claude Code
    /// session refreshed it in between); nothing was written.
    case changedSinceRead
}

/// The credential Claude Code is using right now.
protocol ClaudeCodeCredentialSlot {
    /// nil when the entry or its `claudeAiOauth` object is missing.
    /// Throws `ClaudeCodeSlotError.unreadableEntry` when the entry is not a JSON object.
    func readOAuth() throws -> OAuthCredential?
    /// Replaces `claudeAiOauth` and leaves every other key untouched, but only while the
    /// entry still holds `expecting` (what `readOAuth` returned; nil means the entry or its
    /// `claudeAiOauth` is absent). Otherwise throws `ClaudeCodeSlotError.changedSinceRead`
    /// and writes nothing. A missing entry is created; a corrupt one throws
    /// `ClaudeCodeSlotError.unreadableEntry` and is not modified.
    ///
    ///     let before = try slot.readOAuth()
    ///     try slot.writeOAuth(target, expecting: before)
    func writeOAuth(_ credential: OAuthCredential, expecting: OAuthCredential?) throws
}

/// Claude Code's Keychain entry for the default config dir (`~/.claude`).
///
/// The entry also holds `mcpOAuth` (other MCP servers' tokens); a write must keep it.
///
///     let slot = KeychainClaudeCodeSlot(keychain: SecurityCLIKeychain(), account: NSUserName())
struct KeychainClaudeCodeSlot: ClaudeCodeCredentialSlot {
    static let service = "Claude Code-credentials"

    private let keychain: KeychainStoring
    private let account: String

    /// `account` is explicit: Claude Code keys its entry by the login user name, and a
    /// wrong value would silently read and write a different item.
    init(keychain: KeychainStoring, account: String) {
        self.keychain = keychain
        self.account = account
    }

    func readOAuth() throws -> OAuthCredential? {
        guard let root = try readRoot(),
              let oauth = root["claudeAiOauth"] as? [String: Any] else { return nil }
        return OAuthCredential(object: oauth)
    }

    /// The compare and the write are two `security` calls, not one atomic step: this
    /// narrows the window in which a concurrent refresh is lost to that gap.
    func writeOAuth(_ credential: OAuthCredential, expecting: OAuthCredential?) throws {
        var root = try readRoot() ?? [:]
        let current = (root["claudeAiOauth"] as? [String: Any]).flatMap { OAuthAccountJSON.canonical($0) }
        guard current == expecting?.json else { throw ClaudeCodeSlotError.changedSinceRead }
        root["claudeAiOauth"] = credential.object
        let data = try JSONSerialization.data(withJSONObject: root, options: [.withoutEscapingSlashes])
        try keychain.write(data, service: Self.service, account: account)
    }

    private func readRoot() throws -> [String: Any]? {
        guard let data = try keychain.read(service: Self.service, account: account) else { return nil }
        guard let root = OAuthAccountJSON.object(data) else { throw ClaudeCodeSlotError.unreadableEntry }
        return root
    }
}
