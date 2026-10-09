import Foundation

/// The outcome of asking claude.ai for an authorization code with a `sessionKey`.
enum GrantResult: Equatable {
    /// An authorization code to exchange at the token endpoint.
    case code(String)
    /// claude.ai requires a recent browser sign-in before it will issue a grant
    /// (`session_stale_for_elevated_grant`, or a 403 `session_stale_relogin`). The
    /// silent path cannot proceed; fall back to the browser.
    case stale
}

/// Requests a Claude Code authorization code straight from claude.ai using the account's
/// `sessionKey` cookie, the same consent the browser performs when the user clicks
/// Authorize. This is an undocumented claude.ai endpoint; on any sign of the recent-sign-in
/// gate it returns `.stale` so the caller opens the browser instead.
///
/// Endpoint shape observed on claude.ai, 2026-10-09:
///   GET  /v1/oauth/{org}/authorize?client_id&scope   -> session_stale_for_elevated_grant
///   POST /v1/oauth/{org}/authorize                    -> { redirect_uri: "...?code=...&state=..." }
///
///     let client = ClaudeAIGrantClient()
///     let result = try await client.grantCode(orgId: org, sessionKey: key, pkce: pkce,
///                                              redirectURI: redirect, loginHint: email)
struct ClaudeAIGrantClient {
    static let origin = "https://claude.ai"

    private let session: URLSession

    init(session: URLSession = .shared) {
        self.session = session
    }

    func grantCode(orgId: String, sessionKey: String, pkce: PKCE,
                   redirectURI: String, loginHint: String?) async throws -> GrantResult {
        if try await isStale(orgId: orgId, sessionKey: sessionKey) { return .stale }
        return try await authorize(orgId: orgId, sessionKey: sessionKey, pkce: pkce,
                                   redirectURI: redirectURI, loginHint: loginHint)
    }

    private func authorizeURL(orgId: String) -> URL {
        URL(string: "\(Self.origin)/v1/oauth/\(orgId)/authorize")!
    }

    private func isStale(orgId: String, sessionKey: String) async throws -> Bool {
        var c = URLComponents(url: authorizeURL(orgId: orgId), resolvingAgainstBaseURL: false)!
        c.queryItems = [
            URLQueryItem(name: "client_id", value: ClaudeCodeOAuthClient.clientId),
            URLQueryItem(name: "scope", value: ClaudeCodeOAuthClient.scope),
        ]
        let (object, _) = try await send(request(url: c.url!, sessionKey: sessionKey))
        return object["session_stale_for_elevated_grant"] as? Bool ?? false
    }

    private func authorize(orgId: String, sessionKey: String, pkce: PKCE,
                           redirectURI: String, loginHint: String?) async throws -> GrantResult {
        var req = request(url: authorizeURL(orgId: orgId), sessionKey: sessionKey)
        req.httpMethod = "POST"
        var body: [String: Any] = [
            "response_type": "code",
            "client_id": ClaudeCodeOAuthClient.clientId,
            "organization_uuid": orgId,
            "redirect_uri": redirectURI,
            "scope": ClaudeCodeOAuthClient.scope,
            "state": pkce.state,
            "code_challenge": pkce.challenge,
            "code_challenge_method": "S256",
        ]
        if let loginHint { body["login_hint"] = loginHint }
        req.httpBody = try JSONSerialization.data(withJSONObject: body)

        do {
            let (object, _) = try await send(req)
            guard let redirect = object["redirect_uri"] as? String,
                  let code = Self.code(fromRedirect: redirect) else {
                throw ClaudeCodeOAuthError.malformedResponse
            }
            return .code(code)
        } catch ClaudeCodeOAuthError.http(let status) where status == 403 {
            // A 403 here is the same gate the pre-check missed (the session aged out between
            // the two calls, or the backend is stricter): treat it as stale, not a hard error.
            return .stale
        }
    }

    static func code(fromRedirect redirect: String) -> String? {
        guard let c = URLComponents(string: redirect) else { return nil }
        return c.queryItems?.first { $0.name == "code" }?.value
    }

    private func request(url: URL, sessionKey: String) -> URLRequest {
        var req = URLRequest(url: url)
        req.setValue("*/*", forHTTPHeaderField: "accept")
        req.setValue("application/json", forHTTPHeaderField: "content-type")
        req.setValue("web_claude_ai", forHTTPHeaderField: "anthropic-client-platform")
        req.setValue("sessionKey=\(sessionKey)", forHTTPHeaderField: "Cookie")
        return req
    }

    private func send(_ request: URLRequest) async throws -> ([String: Any], Int) {
        let (data, response) = try await session.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        // A 403 is read for its error_code before it becomes a thrown status.
        if status == 403, let object = OAuthAccountJSON.object(data),
           Self.isStaleRelogin(object) {
            throw ClaudeCodeOAuthError.http(status: 403)
        }
        guard status == 200 else { throw ClaudeCodeOAuthError.http(status: status) }
        guard let object = OAuthAccountJSON.object(data) else { throw ClaudeCodeOAuthError.malformedResponse }
        return (object, status)
    }

    private static func isStaleRelogin(_ object: [String: Any]) -> Bool {
        let error = object["error"] as? [String: Any]
        let details = error?["details"] as? [String: Any]
        return details?["error_code"] as? String == "session_stale_relogin"
    }
}
