import XCTest
@testable import ClaudeDashboard

final class ClaudeAIGrantClientTests: XCTestCase {

    private let pkce = PKCE(verifier: "ver", state: "st")
    private let path = "/v1/oauth/org-1/authorize"

    override func tearDown() {
        MockURLProtocol.requestHandler = nil
        super.tearDown()
    }

    private func makeClient() -> ClaudeAIGrantClient {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        return ClaudeAIGrantClient(session: URLSession(configuration: config))
    }

    private func reply(_ request: URLRequest, _ status: Int, _ body: String) -> (HTTPURLResponse, Data) {
        (HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!, Data(body.utf8))
    }

    func testStalePreCheckReportsStaleWithoutPosting() async throws {
        var sawPost = false
        MockURLProtocol.requestHandler = { request in
            if request.httpMethod == "POST" { sawPost = true }
            XCTAssertEqual(request.value(forHTTPHeaderField: "Cookie"), "sessionKey=sk-1")
            return self.reply(request, 200,
                #"{"client_id":"x","session_stale_for_elevated_grant":true}"#)
        }
        let result = try await makeClient().grantCode(
            orgId: "org-1", sessionKey: "sk-1", pkce: pkce,
            redirectURI: "http://localhost:1/callback", loginHint: "be@x.co")
        XCTAssertEqual(result, .stale)
        XCTAssertFalse(sawPost, "a stale session must not reach the POST")
    }

    func testFreshSessionPostsAndReturnsTheCode() async throws {
        var postBody: [String: Any]?
        MockURLProtocol.requestHandler = { request in
            XCTAssertEqual(request.url?.path, self.path)
            if request.httpMethod == "GET" {
                return self.reply(request, 200, #"{"session_stale_for_elevated_grant":false}"#)
            }
            postBody = try JSONSerialization.jsonObject(with: request.bodyData()) as? [String: Any]
            return self.reply(request, 200,
                #"{"redirect_uri":"http://localhost:1/callback?code=the-code&state=st"}"#)
        }
        let result = try await makeClient().grantCode(
            orgId: "org-1", sessionKey: "sk-1", pkce: pkce,
            redirectURI: "http://localhost:1/callback", loginHint: nil)
        XCTAssertEqual(result, .code("the-code"))
        XCTAssertEqual(postBody?["client_id"] as? String, ClaudeCodeOAuthClient.clientId)
        XCTAssertEqual(postBody?["organization_uuid"] as? String, "org-1")
        XCTAssertEqual(postBody?["response_type"] as? String, "code")
        XCTAssertEqual(postBody?["code_challenge"] as? String, pkce.challenge)
        XCTAssertEqual(postBody?["code_challenge_method"] as? String, "S256")
        XCTAssertEqual(postBody?["state"] as? String, "st")
        XCTAssertEqual(postBody?["redirect_uri"] as? String, "http://localhost:1/callback")
    }

    func testPostRejectedAsStaleReloginReportsStale() async throws {
        MockURLProtocol.requestHandler = { request in
            if request.httpMethod == "GET" {
                return self.reply(request, 200, #"{"session_stale_for_elevated_grant":false}"#)
            }
            return self.reply(request, 403,
                #"{"error":{"type":"permission_error","details":{"error_code":"session_stale_relogin"}}}"#)
        }
        let result = try await makeClient().grantCode(
            orgId: "org-1", sessionKey: "sk-1", pkce: pkce, redirectURI: "r", loginHint: nil)
        XCTAssertEqual(result, .stale)
    }

    func testOtherErrorThrows() async {
        MockURLProtocol.requestHandler = { request in
            self.reply(request, 500, "oops")
        }
        do {
            _ = try await makeClient().grantCode(
                orgId: "org-1", sessionKey: "sk-1", pkce: pkce, redirectURI: "r", loginHint: nil)
            XCTFail("expected a throw")
        } catch {
            XCTAssertEqual(error as? ClaudeCodeOAuthError, .http(status: 500))
        }
    }

    func testMalformedRedirectThrows() async {
        MockURLProtocol.requestHandler = { request in
            if request.httpMethod == "GET" {
                return self.reply(request, 200, #"{"session_stale_for_elevated_grant":false}"#)
            }
            return self.reply(request, 200, #"{"redirect_uri":"http://localhost:1/callback?state=st"}"#)
        }
        do {
            _ = try await makeClient().grantCode(
                orgId: "org-1", sessionKey: "sk-1", pkce: pkce, redirectURI: "r", loginHint: nil)
            XCTFail("expected a throw")
        } catch {
            XCTAssertEqual(error as? ClaudeCodeOAuthError, .malformedResponse)
        }
    }
}
