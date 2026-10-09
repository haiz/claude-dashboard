import Foundation

/// What the provisioner needs to mint a Claude Code login for one account. The
/// `sessionKey` is already decrypted (via `AccountStore.loadSessionKey`).
struct ProvisionInput: Equatable {
    let orgId: String
    let sessionKey: String
    let email: String
    let browser: Browser
    let profilePath: String
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

protocol OAuthCallbackListening {
    func start() throws -> Int
    func waitForCode(timeout: TimeInterval) async throws -> String
    func cancel()
}

extension ClaudeAIGrantClient: ClaudeAIGrantRequesting {}
extension ClaudeCodeOAuthClient: ClaudeCodeTokenExchanging {}
extension OAuthCallbackListener: OAuthCallbackListening {}

/// Mints a Claude Code login for an account without the user running `/login`.
///
/// It first tries the silent path: ask claude.ai for a code with the account's
/// `sessionKey` and exchange it. If claude.ai requires a recent sign-in (`.stale`), it
/// opens the account's browser profile on the consent page and waits on a loopback
/// listener for the code the browser redirects back. Either way it verifies the minted
/// login's email matches the account before returning it.
///
///     let entry = try await provisioner.provision(input) { showWaitingForBrowserUI() }
///     try vault.save(entry, for: account.id)
final class ClaudeCodeLoginProvisioner {
    private let grantClient: ClaudeAIGrantRequesting
    private let oauthClient: ClaudeCodeTokenExchanging
    private let makeListener: (String) -> OAuthCallbackListening
    private let openBrowser: (Browser, String, URL) throws -> Void
    private let browserTimeout: TimeInterval

    init(grantClient: ClaudeAIGrantRequesting,
         oauthClient: ClaudeCodeTokenExchanging,
         makeListener: @escaping (String) -> OAuthCallbackListening,
         openBrowser: @escaping (Browser, String, URL) throws -> Void,
         browserTimeout: TimeInterval) {
        self.grantClient = grantClient
        self.oauthClient = oauthClient
        self.makeListener = makeListener
        self.openBrowser = openBrowser
        self.browserTimeout = browserTimeout
    }

    static func live() -> ClaudeCodeLoginProvisioner {
        let opener = BrowserProfileOpener()
        return ClaudeCodeLoginProvisioner(
            grantClient: ClaudeAIGrantClient(),
            oauthClient: ClaudeCodeOAuthClient(),
            makeListener: { OAuthCallbackListener(expectedState: $0) },
            openBrowser: { try opener.open(browser: $0, profilePath: $1, url: $2) },
            browserTimeout: 300)
    }

    /// `onAwaitingBrowser` fires only when the silent path fails and the browser opens,
    /// so the UI can show "waiting for the browser" with a cancel.
    func provision(_ input: ProvisionInput, onAwaitingBrowser: () -> Void) async throws -> VaultEntry {
        let pkce = PKCE.make()
        if let entry = try await silentEntry(input, pkce: pkce) {
            try verify(entry, expected: input.email)
            return entry
        }
        let entry = try await browserFlow(input, pkce: pkce, onAwaitingBrowser: onAwaitingBrowser)
        try verify(entry, expected: input.email)
        return entry
    }

    /// The silent path only: a minted entry, or nil when claude.ai wants a browser sign-in.
    /// Used on refresh to capture accounts the user has recently signed into, with no UI.
    func provisionSilently(_ input: ProvisionInput) async throws -> VaultEntry? {
        guard let entry = try await silentEntry(input, pkce: PKCE.make()) else { return nil }
        try verify(entry, expected: input.email)
        return entry
    }

    private func silentEntry(_ input: ProvisionInput, pkce: PKCE) async throws -> VaultEntry? {
        let grant = try await grantClient.grantCode(
            orgId: input.orgId, sessionKey: input.sessionKey, pkce: pkce,
            redirectURI: ClaudeCodeOAuthClient.manualRedirectURI, loginHint: input.email)
        guard case .code(let code) = grant else { return nil }
        return try await oauthClient.exchange(
            code: code, pkce: pkce, redirectURI: ClaudeCodeOAuthClient.manualRedirectURI)
    }

    private func browserFlow(_ input: ProvisionInput, pkce: PKCE,
                             onAwaitingBrowser: () -> Void) async throws -> VaultEntry {
        let listener = makeListener(pkce.state)
        let port = try listener.start()
        defer { listener.cancel() }
        // `localhost`, not `127.0.0.1`: this is the redirect Claude Code's client registers,
        // and the token endpoint matches redirect_uri as an exact string.
        let redirectURI = "http://localhost:\(port)/callback"
        let url = ClaudeCodeOAuthClient.authorizeURL(pkce: pkce, redirectURI: redirectURI, loginHint: input.email)

        try openBrowser(input.browser, input.profilePath, url)
        onAwaitingBrowser()

        // Cancelling the surrounding Task (the UI's Cancel) wakes the wait via the listener.
        let code = try await withTaskCancellationHandler {
            try await listener.waitForCode(timeout: browserTimeout)
        } onCancel: {
            listener.cancel()
        }
        return try await oauthClient.exchange(code: code, pkce: pkce, redirectURI: redirectURI)
    }

    private func verify(_ entry: VaultEntry, expected: String) throws {
        let got = OAuthAccountJSON.email(entry.oauthAccount) ?? ""
        guard got.caseInsensitiveCompare(expected) == .orderedSame else {
            throw ProvisionError.emailMismatch(expected: expected, got: got)
        }
    }
}
