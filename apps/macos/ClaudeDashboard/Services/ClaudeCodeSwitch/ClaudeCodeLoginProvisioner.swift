import Foundation

/// What the provisioner needs to mint a Claude Code login for one account. The
/// `sessionKey` is already decrypted (via `AccountStore.loadSessionKey`).
struct ProvisionInput: Equatable {
    let orgId: String
    let sessionKey: String
    let email: String
}

enum ProvisionError: Error, Equatable {
    /// The minted login belongs to a different account than the one asked for. The entry
    /// is discarded so one account's Switch can never install another's credential.
    case emailMismatch(expected: String, got: String)
}

protocol ClaudeAIGrantRequesting {
    func grantCode(orgId: String, sessionKey: String, pkce: PKCE,
                   redirectURI: String, loginHint: String?) async throws -> GrantResult
}

protocol ClaudeCodeTokenExchanging {
    func exchange(code: String, pkce: PKCE, redirectURI: String) async throws -> VaultEntry
}

extension ClaudeAIGrantClient: ClaudeAIGrantRequesting {}
extension ClaudeCodeOAuthClient: ClaudeCodeTokenExchanging {}

/// Mints a Claude Code login for an account without the user running `/login`, using the
/// account's claude.ai `sessionKey` and Claude Code's own OAuth client.
///
/// Silent only, on purpose: claude.ai grants a Claude Code token only when the browser
/// signed in recently (`session_stale_for_elevated_grant`). When the session is stale this
/// returns nil — the dashboard does not open a browser mid-switch (for shared/group-email
/// accounts the login is an email magic link, a poor interruption). The user signs in to
/// claude.ai normally in the browser; the next refresh then captures the account silently.
///
///     if let entry = try await provisioner.provisionSilently(input) {
///         try vault.save(entry, for: account.id)   // one-click switch for ~30 days
///     }   // nil: session stale -> ask the user to sign in to claude.ai, then retry
final class ClaudeCodeLoginProvisioner {
    private let grantClient: ClaudeAIGrantRequesting
    private let oauthClient: ClaudeCodeTokenExchanging

    init(grantClient: ClaudeAIGrantRequesting, oauthClient: ClaudeCodeTokenExchanging) {
        self.grantClient = grantClient
        self.oauthClient = oauthClient
    }

    static func live() -> ClaudeCodeLoginProvisioner {
        ClaudeCodeLoginProvisioner(grantClient: ClaudeAIGrantClient(), oauthClient: ClaudeCodeOAuthClient())
    }

    /// A minted, email-verified entry, or nil when claude.ai wants a fresh browser sign-in.
    func provisionSilently(_ input: ProvisionInput) async throws -> VaultEntry? {
        let pkce = PKCE.make()
        let grant = try await grantClient.grantCode(
            orgId: input.orgId, sessionKey: input.sessionKey, pkce: pkce,
            redirectURI: ClaudeCodeOAuthClient.manualRedirectURI, loginHint: input.email)
        guard case .code(let code) = grant else { return nil }
        let entry = try await oauthClient.exchange(
            code: code, pkce: pkce, redirectURI: ClaudeCodeOAuthClient.manualRedirectURI)
        try verify(entry, expected: input.email)
        return entry
    }

    private func verify(_ entry: VaultEntry, expected: String) throws {
        let got = OAuthAccountJSON.email(entry.oauthAccount) ?? ""
        guard got.caseInsensitiveCompare(expected) == .orderedSame else {
            throw ProvisionError.emailMismatch(expected: expected, got: got)
        }
    }
}
