import Foundation

/// The credential Claude Code is using right now.
protocol ClaudeCodeCredentialSlot {
    /// nil when the entry or its `claudeAiOauth` key is missing.
    func readOAuth() throws -> OAuthCredential?
    /// Replaces `claudeAiOauth` and leaves every other key untouched.
    func writeOAuth(_ credential: OAuthCredential) throws
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

    func writeOAuth(_ credential: OAuthCredential) throws {
        var root = try readRoot() ?? [:]
        root["claudeAiOauth"] = credential.object
        let data = try JSONSerialization.data(withJSONObject: root, options: [.withoutEscapingSlashes])
        try keychain.write(data, service: Self.service, account: account)
    }

    private func readRoot() throws -> [String: Any]? {
        guard let data = try keychain.read(service: Self.service, account: account) else { return nil }
        return OAuthAccountJSON.object(data)
    }
}
