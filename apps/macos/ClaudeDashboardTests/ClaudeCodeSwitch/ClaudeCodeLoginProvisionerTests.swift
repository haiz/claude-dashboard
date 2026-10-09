import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeLoginProvisionerTests: XCTestCase {

    // MARK: Fakes

    private final class FakeGrant: ClaudeAIGrantRequesting {
        var result: GrantResult = .stale
        var lastRedirect: String?
        func grantCode(orgId: String, sessionKey: String, pkce: PKCE,
                       redirectURI: String, loginHint: String?) async throws -> GrantResult {
            lastRedirect = redirectURI
            return result
        }
    }

    private final class FakeExchange: ClaudeCodeTokenExchanging {
        var email = "be@x.co"
        var codes: [String] = []
        func exchange(code: String, pkce: PKCE, redirectURI: String) async throws -> VaultEntry {
            codes.append(code)
            return VaultEntry(
                oauth: OAuthCredential(object: ["refreshToken": "rt-\(code)"])!,
                oauthAccount: OAuthAccountJSON.canonical(["emailAddress": email])!)
        }
    }

    private func makeProvisioner(_ grant: FakeGrant, _ exchange: FakeExchange) -> ClaudeCodeLoginProvisioner {
        ClaudeCodeLoginProvisioner(grantClient: grant, oauthClient: exchange)
    }

    private let input = ProvisionInput(orgId: "org-1", sessionKey: "sk-1", email: "be@x.co")

    // MARK: Tests

    func testMintsWhenTheSessionIsFresh() async throws {
        let grant = FakeGrant(); grant.result = .code("silent-code")
        let exchange = FakeExchange()
        let entry = try await makeProvisioner(grant, exchange).provisionSilently(input)
        XCTAssertEqual(exchange.codes, ["silent-code"])
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(entry).oauthAccount), "be@x.co")
        // The grant and its exchange must agree on redirect_uri.
        XCTAssertEqual(grant.lastRedirect, ClaudeCodeOAuthClient.manualRedirectURI)
    }

    func testReturnsNilWhenTheSessionIsStale() async throws {
        let grant = FakeGrant(); grant.result = .stale
        let exchange = FakeExchange()
        let entry = try await makeProvisioner(grant, exchange).provisionSilently(input)
        XCTAssertNil(entry)
        XCTAssertTrue(exchange.codes.isEmpty, "a stale grant never reaches the token exchange")
    }

    func testRejectsAMintedLoginForAnotherAccount() async {
        let grant = FakeGrant(); grant.result = .code("silent-code")
        let exchange = FakeExchange(); exchange.email = "someone.else@x.co"
        do {
            _ = try await makeProvisioner(grant, exchange).provisionSilently(input)
            XCTFail("expected a mismatch throw")
        } catch {
            XCTAssertEqual(error as? ProvisionError, .emailMismatch(expected: "be@x.co", got: "someone.else@x.co"))
        }
    }

    func testEmailMatchIsCaseInsensitive() async throws {
        let grant = FakeGrant(); grant.result = .code("c")
        let exchange = FakeExchange(); exchange.email = "BE@X.CO"
        let entry = try await makeProvisioner(grant, exchange).provisionSilently(input)
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(entry).oauthAccount), "BE@X.CO")
    }
}
