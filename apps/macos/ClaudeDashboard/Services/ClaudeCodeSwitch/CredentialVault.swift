import Foundation

/// The app's per-account copies of Claude Code credentials.
protocol CredentialVaulting {
    func load(_ accountId: UUID) throws -> VaultEntry?
    func save(_ entry: VaultEntry, for accountId: UUID) throws
}

/// One Keychain item per dashboard account, holding
/// `{"claudeAiOauth": {...}, "oauthAccount": {...}}`.
///
///     let vault = KeychainCredentialVault(keychain: SecurityCLIKeychain())
///     try vault.save(entry, for: account.id)
struct KeychainCredentialVault: CredentialVaulting {
    static let service = "ClaudeDashboard.cc-vault"

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
}
