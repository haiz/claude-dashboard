import CryptoKit
import Foundation

/// A PKCE pair plus the OAuth `state`, as Claude Code makes them: 32 random bytes each,
/// base64url without padding; the challenge is base64url(SHA-256(verifier)).
struct PKCE: Equatable {
    let verifier: String
    let state: String

    var challenge: String { Self.base64url(Data(SHA256.hash(data: Data(verifier.utf8)))) }

    static func make() -> PKCE {
        PKCE(verifier: base64url(randomBytes()), state: base64url(randomBytes()))
    }

    private static func randomBytes() -> Data {
        Data((0..<32).map { _ in UInt8.random(in: .min ... .max) })
    }

    static func base64url(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

enum ClaudeCodeOAuthError: Error, Equatable {
    case http(status: Int)
    case malformedResponse
}

/// Claude Code's own OAuth client (constants read from the Claude Code 2.1.295 binary):
/// builds the authorize URL `/login` opens, and turns an authorization code into the
/// `claudeAiOauth` + `oauthAccount` pair `/login` stores.
///
///     let pkce = PKCE.make()
///     let url = ClaudeCodeOAuthClient.authorizeURL(pkce: pkce, redirectURI: redirect, loginHint: email)
///     // ... the browser (or ClaudeAIGrantClient) yields `code` ...
///     let entry = try await ClaudeCodeOAuthClient().exchange(code: code, pkce: pkce, redirectURI: redirect)
struct ClaudeCodeOAuthClient {
    static let clientId = "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
    static let authorizeEndpoint = "https://claude.com/cai/oauth/authorize"
    static let tokenURL = URL(string: "https://platform.claude.com/v1/oauth/token")!
    static let profileURL = URL(string: "https://api.anthropic.com/api/oauth/profile")!
    /// Where a code goes when no local listener runs; also accepted by the token endpoint.
    static let manualRedirectURI = "https://platform.claude.com/oauth/code/callback"
    static let scope = "user:profile user:inference user:sessions:claude_code user:mcp_servers "
        + "user:file_upload user:plugins"
    /// Claude Code's fallback when the token response has no `refresh_token_expires_in`.
    static let defaultRefreshLifetime: TimeInterval = 30 * 86400

    private let session: URLSession
    private let now: () -> Date

    init(session: URLSession = .shared, now: @escaping () -> Date = Date.init) {
        self.session = session
        self.now = now
    }

    static func authorizeURL(pkce: PKCE, redirectURI: String, loginHint: String?) -> URL {
        var c = URLComponents(string: authorizeEndpoint)!
        c.queryItems = [
            URLQueryItem(name: "code", value: "true"),
            URLQueryItem(name: "client_id", value: clientId),
            URLQueryItem(name: "response_type", value: "code"),
            URLQueryItem(name: "redirect_uri", value: redirectURI),
            URLQueryItem(name: "scope", value: scope),
            URLQueryItem(name: "code_challenge", value: pkce.challenge),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
            URLQueryItem(name: "state", value: pkce.state),
        ] + (loginHint.map { [URLQueryItem(name: "login_hint", value: $0)] } ?? [])
        return c.url!
    }

    /// Token exchange, then the profile call that supplies `subscriptionType`,
    /// `rateLimitTier` and `oauthAccount`.
    func exchange(code: String, pkce: PKCE, redirectURI: String) async throws -> VaultEntry {
        let token = try await post(Self.tokenURL, body: [
            "grant_type": "authorization_code", "code": code, "redirect_uri": redirectURI,
            "client_id": Self.clientId, "code_verifier": pkce.verifier, "state": pkce.state,
        ])
        guard let accessToken = token["access_token"] as? String,
              let refreshToken = token["refresh_token"] as? String,
              let expiresIn = (token["expires_in"] as? NSNumber)?.doubleValue else {
            throw ClaudeCodeOAuthError.malformedResponse
        }
        let profile = try await fetchProfile(accessToken: accessToken)
        let account = profile["account"] as? [String: Any] ?? [:]
        let org = profile["organization"] as? [String: Any] ?? [:]
        guard account["email"] is String else { throw ClaudeCodeOAuthError.malformedResponse }

        let issued = now()
        let refreshLifetime = (token["refresh_token_expires_in"] as? NSNumber)?.doubleValue
            ?? Self.defaultRefreshLifetime
        var oauth: [String: Any] = [
            "accessToken": accessToken,
            "refreshToken": refreshToken,
            "expiresAt": Self.millis(issued.addingTimeInterval(expiresIn)),
            "refreshTokenExpiresAt": Self.millis(issued.addingTimeInterval(refreshLifetime)),
            "scopes": (token["scope"] as? String)?.split(separator: " ").map(String.init) ?? [],
        ]
        oauth["subscriptionType"] = (org["organization_type"] as? String).flatMap(Self.subscriptionType)
        oauth["rateLimitTier"] = org["rate_limit_tier"] as? String

        var oauthAccount: [String: Any] = [
            "accountUuid": account["uuid"] ?? NSNull(),
            "emailAddress": account["email"]!,
            "organizationUuid": org["uuid"] ?? NSNull(),
            "profileFetchedAt": Self.millis(issued),
        ]
        let copies: [(String, [String: Any], String)] = [
            ("displayName", account, "display_name"), ("fullName", account, "full_name"),
            ("accountCreatedAt", account, "created_at"), ("organizationName", org, "name"),
            ("organizationType", org, "organization_type"), ("billingType", org, "billing_type"),
            ("organizationRateLimitTier", org, "rate_limit_tier"), ("seatTier", org, "seat_tier"),
            ("hasExtraUsageEnabled", org, "has_extra_usage_enabled"),
            ("subscriptionCreatedAt", org, "subscription_created_at"),
        ]
        for (key, source, field) in copies { oauthAccount[key] = source[field] }

        guard let credential = OAuthCredential(object: oauth),
              let accountJSON = OAuthAccountJSON.canonical(oauthAccount) else {
            throw ClaudeCodeOAuthError.malformedResponse
        }
        return VaultEntry(oauth: credential, oauthAccount: accountJSON)
    }

    /// Claude Code's mapping of `organization.organization_type`.
    static func subscriptionType(_ organizationType: String) -> String? {
        ["claude_max": "max", "claude_pro": "pro", "claude_enterprise": "enterprise",
         "claude_team": "team"][organizationType]
    }

    private static func millis(_ date: Date) -> Int64 { Int64(date.timeIntervalSince1970 * 1000) }

    private func post(_ url: URL, body: [String: Any]) async throws -> [String: Any] {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        return try await send(request)
    }

    private func fetchProfile(accessToken: String) async throws -> [String: Any] {
        var request = URLRequest(url: Self.profileURL)
        request.setValue("Bearer \(accessToken)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("no-cache", forHTTPHeaderField: "Cache-Control")
        return try await send(request)
    }

    private func send(_ request: URLRequest) async throws -> [String: Any] {
        let (data, response) = try await session.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else { throw ClaudeCodeOAuthError.http(status: status) }
        guard let object = OAuthAccountJSON.object(data) else { throw ClaudeCodeOAuthError.malformedResponse }
        return object
    }
}
