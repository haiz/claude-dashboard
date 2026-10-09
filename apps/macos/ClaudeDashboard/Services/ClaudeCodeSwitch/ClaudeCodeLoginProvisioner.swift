import Foundation

/// What the provisioner needs to mint a Claude Code login for one account by opening the
/// account's browser profile on Claude's consent page.
struct ProvisionInput: Equatable {
    let email: String
    let browser: Browser
    let profilePath: String
}

enum ProvisionError: Error, Equatable {
    /// The minted login belongs to a different account than the one asked for. The entry
    /// is discarded so one account's Switch can never install another's credential.
    case emailMismatch(expected: String, got: String)
}

protocol ClaudeCodeTokenExchanging {
    func exchange(code: String, pkce: PKCE, redirectURI: String) async throws -> VaultEntry
}

protocol OAuthCallbackListening {
    func start() throws -> Int
    func waitForCode(timeout: TimeInterval) async throws -> String
    func cancel()
}

extension ClaudeCodeOAuthClient: ClaudeCodeTokenExchanging {}
extension OAuthCallbackListener: OAuthCallbackListening {}

/// Mints a Claude Code login for an account without the user running `/login`, by opening
/// that account's **browser profile** on Claude's own OAuth consent page and catching the
/// redirect on a loopback listener.
///
/// Browser only, on purpose: claude.ai grants a Claude Code token only after a recent
/// interactive sign-in, so there is no reliable way to mint one silently. Opening the exact
/// profile means the consent runs under the right claude.ai session, so the minted login is
/// unambiguously that account's (verified by email afterwards). The user completes any
/// sign-in Claude asks for (magic link / SSO) in that tab; the code comes back to the
/// listener and is exchanged for the `claudeAiOauth` + `oauthAccount` pair `/login` stores.
///
///     let entry = try await provisioner.provision(input) { showWaitingForBrowserUI() }
///     try vault.save(entry, for: account.id)
final class ClaudeCodeLoginProvisioner {
    private let oauthClient: ClaudeCodeTokenExchanging
    private let makeListener: (String) -> OAuthCallbackListening
    private let openBrowser: (Browser, String, URL) throws -> Void
    private let browserTimeout: TimeInterval

    init(oauthClient: ClaudeCodeTokenExchanging,
         makeListener: @escaping (String) -> OAuthCallbackListening,
         openBrowser: @escaping (Browser, String, URL) throws -> Void,
         browserTimeout: TimeInterval) {
        self.oauthClient = oauthClient
        self.makeListener = makeListener
        self.openBrowser = openBrowser
        self.browserTimeout = browserTimeout
    }

    static func live() -> ClaudeCodeLoginProvisioner {
        let opener = BrowserProfileOpener()
        return ClaudeCodeLoginProvisioner(
            oauthClient: ClaudeCodeOAuthClient(),
            makeListener: { OAuthCallbackListener(expectedState: $0) },
            openBrowser: { try opener.open(browser: $0, profilePath: $1, url: $2) },
            browserTimeout: 300)
    }

    /// `onAwaitingBrowser` fires once the browser is opened, so the UI can show a cancellable
    /// "finish in your browser" state. Cancelling the surrounding Task wakes the wait.
    func provision(_ input: ProvisionInput, onAwaitingBrowser: () -> Void) async throws -> VaultEntry {
        let pkce = PKCE.make()
        let listener = makeListener(pkce.state)
        let port = try listener.start()
        defer { listener.cancel() }
        // `localhost`, not `127.0.0.1`: this is the redirect Claude Code's client registers,
        // and the token endpoint matches redirect_uri as an exact string.
        let redirectURI = "http://localhost:\(port)/callback"
        let url = ClaudeCodeOAuthClient.authorizeURL(pkce: pkce, redirectURI: redirectURI, loginHint: input.email)

        try openBrowser(input.browser, input.profilePath, url)
        onAwaitingBrowser()

        let code = try await withTaskCancellationHandler {
            try await listener.waitForCode(timeout: browserTimeout)
        } onCancel: {
            listener.cancel()
        }
        let entry = try await oauthClient.exchange(code: code, pkce: pkce, redirectURI: redirectURI)
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
