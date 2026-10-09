import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeOAuthClientTests: XCTestCase {

    private let fixedNow = Date(timeIntervalSince1970: 1_800_000_000)

    override func tearDown() {
        MockURLProtocol.requestHandler = nil
        super.tearDown()
    }

    private func makeClient() -> ClaudeCodeOAuthClient {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        return ClaudeCodeOAuthClient(session: URLSession(configuration: config), now: { self.fixedNow })
    }

    func testPKCEChallengeIsSHA256OfVerifier() {
        // RFC 7636 appendix B.
        let pkce = PKCE(verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", state: "s")
        XCTAssertEqual(pkce.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
        let random = PKCE.make()
        XCTAssertEqual(random.verifier.count, 43)
        XCTAssertNotEqual(random.verifier, random.state)
    }

    func testExchangeBuildsVaultEntryLikeClaudeCode() async throws {
        var tokenBody: [String: Any]?
        var profileAuth: String?
        MockURLProtocol.requestHandler = { request in
            let ok = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            switch request.url?.absoluteString {
            case "https://platform.claude.com/v1/oauth/token":
                XCTAssertEqual(request.httpMethod, "POST")
                tokenBody = try JSONSerialization.jsonObject(with: request.bodyData()) as? [String: Any]
                return (ok, Data("""
                {"access_token":"at","refresh_token":"rt","expires_in":28800,
                 "refresh_token_expires_in":2592000,"scope":"user:inference user:profile","token_type":"Bearer"}
                """.utf8))
            case "https://api.anthropic.com/api/oauth/profile":
                profileAuth = request.value(forHTTPHeaderField: "Authorization")
                return (ok, Data("""
                {"account":{"uuid":"acc-1","email":"be@x.co","display_name":"Be","full_name":"B E",
                  "created_at":"2025-01-01T00:00:00Z"},
                 "organization":{"uuid":"org-1","name":"Org","organization_type":"claude_team",
                  "rate_limit_tier":"default_claude_max_5x","billing_type":"stripe_subscription",
                  "seat_tier":"team_tier_1","has_extra_usage_enabled":true,
                  "subscription_created_at":"2025-02-01T00:00:00Z"}}
                """.utf8))
            default:
                XCTFail("unexpected \(request.url!)")
                throw URLError(.badURL)
            }
        }

        let pkce = PKCE(verifier: "ver", state: "st")
        let entry = try await makeClient().exchange(code: "c0de", pkce: pkce, redirectURI: "http://localhost:1/callback")

        XCTAssertEqual(tokenBody?["grant_type"] as? String, "authorization_code")
        XCTAssertEqual(tokenBody?["code"] as? String, "c0de")
        XCTAssertEqual(tokenBody?["redirect_uri"] as? String, "http://localhost:1/callback")
        XCTAssertEqual(tokenBody?["client_id"] as? String, ClaudeCodeOAuthClient.clientId)
        XCTAssertEqual(tokenBody?["code_verifier"] as? String, "ver")
        XCTAssertEqual(tokenBody?["state"] as? String, "st")
        XCTAssertEqual(profileAuth, "Bearer at")

        let oauth = try XCTUnwrap(OAuthAccountJSON.object(entry.oauth.json))
        XCTAssertEqual(oauth["accessToken"] as? String, "at")
        XCTAssertEqual(oauth["refreshToken"] as? String, "rt")
        XCTAssertEqual(entry.oauth.expiresAt, fixedNow.addingTimeInterval(28800))
        XCTAssertEqual(entry.oauth.refreshTokenExpiresAt, fixedNow.addingTimeInterval(2_592_000))
        XCTAssertEqual(oauth["scopes"] as? [String], ["user:inference", "user:profile"])
        XCTAssertEqual(oauth["subscriptionType"] as? String, "team")
        XCTAssertEqual(oauth["rateLimitTier"] as? String, "default_claude_max_5x")

        let account = try XCTUnwrap(OAuthAccountJSON.object(entry.oauthAccount))
        XCTAssertEqual(account["emailAddress"] as? String, "be@x.co")
        XCTAssertEqual(account["accountUuid"] as? String, "acc-1")
        XCTAssertEqual(account["organizationUuid"] as? String, "org-1")
        XCTAssertEqual(account["organizationName"] as? String, "Org")
        XCTAssertEqual(account["seatTier"] as? String, "team_tier_1")
    }

    func testMissingRefreshExpiryDefaultsToThirtyDays() async throws {
        MockURLProtocol.requestHandler = { request in
            let ok = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            if request.url?.host == "platform.claude.com" {
                return (ok, Data(#"{"access_token":"at","refresh_token":"rt","expires_in":60,"scope":"user:profile"}"#.utf8))
            }
            return (ok, Data(#"{"account":{"uuid":"a","email":"be@x.co"},"organization":{"uuid":"o"}}"#.utf8))
        }
        let entry = try await makeClient().exchange(code: "c", pkce: PKCE(verifier: "v", state: "s"), redirectURI: "r")
        XCTAssertEqual(entry.oauth.refreshTokenExpiresAt, fixedNow.addingTimeInterval(30 * 86400))
    }

    func testTokenErrorThrowsHTTPStatus() async {
        MockURLProtocol.requestHandler = { request in
            (HTTPURLResponse(url: request.url!, statusCode: 400, httpVersion: nil, headerFields: nil)!,
             Data(#"{"error":"invalid_grant"}"#.utf8))
        }
        do {
            _ = try await makeClient().exchange(code: "c", pkce: PKCE(verifier: "v", state: "s"), redirectURI: "r")
            XCTFail("expected a throw")
        } catch {
            XCTAssertEqual(error as? ClaudeCodeOAuthError, .http(status: 400))
        }
    }
}

extension URLRequest {
    /// URLProtocol sees the body as a stream, not `httpBody`.
    func bodyData() -> Data {
        if let httpBody { return httpBody }
        guard let stream = httpBodyStream else { return Data() }
        stream.open()
        defer { stream.close() }
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while stream.hasBytesAvailable {
            let n = stream.read(&buffer, maxLength: buffer.count)
            if n <= 0 { break }
            data.append(buffer, count: n)
        }
        return data
    }
}
