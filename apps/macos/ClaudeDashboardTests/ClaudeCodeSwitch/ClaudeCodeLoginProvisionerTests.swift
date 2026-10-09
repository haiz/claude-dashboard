import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeLoginProvisionerTests: XCTestCase {

    // MARK: Fakes

    private final class FakeGrant: ClaudeAIGrantRequesting {
        var result: GrantResult = .stale
        var calls = 0
        var lastRedirect: String?
        func grantCode(orgId: String, sessionKey: String, pkce: PKCE,
                       redirectURI: String, loginHint: String?) async throws -> GrantResult {
            calls += 1
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

    private final class FakeListener: OAuthCallbackListening {
        var code = "browser-code"
        func start() throws -> Int { 4321 }
        func waitForCode(timeout: TimeInterval) async throws -> String { code }
        func cancel() {}
    }

    private func makeProvisioner(_ grant: FakeGrant, _ exchange: FakeExchange,
                                 listener: FakeListener = FakeListener(),
                                 opened: @escaping (URL) -> Void = { _ in }) -> ClaudeCodeLoginProvisioner {
        ClaudeCodeLoginProvisioner(
            grantClient: grant, oauthClient: exchange,
            makeListener: { _ in listener },
            openBrowser: { _, _, url in opened(url) },
            browserTimeout: 1)
    }

    private let input = ProvisionInput(
        orgId: "org-1", sessionKey: "sk-1", email: "be@x.co", browser: .chrome, profilePath: "Profile 1")

    // MARK: Tests

    func testSilentPathExchangesWithoutOpeningBrowser() async throws {
        let grant = FakeGrant(); grant.result = .code("silent-code")
        let exchange = FakeExchange()
        var opened = false
        let provisioner = makeProvisioner(grant, exchange, opened: { _ in opened = true })

        var awaited = false
        let entry = try await provisioner.provision(input) { awaited = true }

        XCTAssertEqual(exchange.codes, ["silent-code"])
        XCTAssertFalse(opened)
        XCTAssertFalse(awaited, "onAwaitingBrowser must not fire on the silent path")
        XCTAssertEqual(OAuthAccountJSON.email(entry.oauthAccount), "be@x.co")
        // The silent grant and its exchange must agree on redirect_uri.
        XCTAssertEqual(grant.lastRedirect, ClaudeCodeOAuthClient.manualRedirectURI)
    }

    func testStaleSessionFallsBackToBrowser() async throws {
        let grant = FakeGrant(); grant.result = .stale
        let exchange = FakeExchange()
        let listener = FakeListener(); listener.code = "browser-code"
        var openedURL: URL?
        let provisioner = makeProvisioner(grant, exchange, listener: listener, opened: { openedURL = $0 })

        var awaited = false
        let entry = try await provisioner.provision(input) { awaited = true }

        XCTAssertTrue(awaited, "onAwaitingBrowser fires when the browser opens")
        XCTAssertEqual(exchange.codes, ["browser-code"])
        XCTAssertEqual(OAuthAccountJSON.email(entry.oauthAccount), "be@x.co")
        // The browser opens the authorize page on the loopback redirect.
        let query = try XCTUnwrap(URLComponents(url: XCTUnwrap(openedURL), resolvingAgainstBaseURL: false)?.queryItems)
        let redirect = query.first { $0.name == "redirect_uri" }?.value
        XCTAssertEqual(redirect, "http://localhost:4321/callback")
        XCTAssertEqual(query.first { $0.name == "login_hint" }?.value, "be@x.co")
    }

    func testEmailMismatchRejectsTheGrant() async {
        let grant = FakeGrant(); grant.result = .code("silent-code")
        let exchange = FakeExchange(); exchange.email = "someone.else@x.co"
        let provisioner = makeProvisioner(grant, exchange)
        do {
            _ = try await provisioner.provision(input) { }
            XCTFail("expected a mismatch throw")
        } catch {
            XCTAssertEqual(error as? ProvisionError, .emailMismatch(expected: "be@x.co", got: "someone.else@x.co"))
        }
    }

    func testProvisionSilentlyReturnsNilWhenStale() async throws {
        let grant = FakeGrant(); grant.result = .stale
        let exchange = FakeExchange()
        var opened = false
        let provisioner = makeProvisioner(grant, exchange, opened: { _ in opened = true })
        let entry = try await provisioner.provisionSilently(input)
        XCTAssertNil(entry)
        XCTAssertFalse(opened, "the silent path never opens a browser")
    }

    func testProvisionSilentlyMintsWhenFresh() async throws {
        let grant = FakeGrant(); grant.result = .code("silent-code")
        let exchange = FakeExchange()
        let provisioner = makeProvisioner(grant, exchange)
        let entry = try await provisioner.provisionSilently(input)
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(entry).oauthAccount), "be@x.co")
        XCTAssertEqual(exchange.codes, ["silent-code"])
    }

    func testEmailMatchIsCaseInsensitive() async throws {
        let grant = FakeGrant(); grant.result = .code("c")
        let exchange = FakeExchange(); exchange.email = "BE@X.CO"
        let provisioner = makeProvisioner(grant, exchange)
        let entry = try await provisioner.provision(input) { }
        XCTAssertEqual(OAuthAccountJSON.email(entry.oauthAccount), "BE@X.CO")
    }
}
