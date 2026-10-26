import Foundation

/// The app's per-account copies of Claude Desktop credentials.
///
/// Same layout and behavior as `KeychainCredentialVault`, under its own Keychain service so
/// Claude Code and Claude Desktop keep independent refresh-token chains per account.
///
///     let vault = KeychainDesktopVault(keychain: SecurityCLIKeychain())
///     try vault.save(entry, for: account.id)
///     try vault.delete(account.id)   // when the account is removed
struct KeychainDesktopVault: CredentialVaulting {
    static let service = "ClaudeDashboard.desktop-vault"

    private let keychain: KeychainStoring

    init(keychain: KeychainStoring) {
        self.keychain = keychain
    }

    func load(_ accountId: UUID) throws -> VaultEntry? {
        guard let data = try keychain.read(service: Self.service, account: accountId.uuidString),
              let root = OAuthAccountJSON.object(data),
              let oauthObject = root["claudeAiOauth"] as? [String: Any],
              let oauth = OAuthCredential(object: oauthObject),
              let accountObject = root["oauthAccount"],
              let oauthAccount = OAuthAccountJSON.canonical(accountObject) else { return nil }
        return VaultEntry(oauth: oauth, oauthAccount: oauthAccount)
    }

    func save(_ entry: VaultEntry, for accountId: UUID) throws {
        let root: [String: Any] = [
            "claudeAiOauth": entry.oauth.object,
            "oauthAccount": OAuthAccountJSON.object(entry.oauthAccount) ?? [:],
        ]
        let data = try JSONSerialization.data(withJSONObject: root, options: [.sortedKeys, .withoutEscapingSlashes])
        try keychain.write(data, service: Self.service, account: accountId.uuidString)
    }

    func delete(_ accountId: UUID) throws {
        try keychain.delete(service: Self.service, account: accountId.uuidString)
    }
}
