import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeLoginProvisionerTests: XCTestCase {

    private final class FakeExchange: ClaudeCodeTokenExchanging {
        var email = "be@x.co"
        var codes: [String] = []
        var redirects: [String] = []
        func exchange(code: String, pkce: PKCE, redirectURI: String) async throws -> VaultEntry {
            codes.append(code); redirects.append(redirectURI)
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

    private func makeProvisioner(_ exchange: FakeExchange, listener: FakeListener = FakeListener(),
                                 opened: @escaping (Browser, String, URL) -> Void = { _, _, _ in })
        -> ClaudeCodeLoginProvisioner {
        ClaudeCodeLoginProvisioner(
            oauthClient: exchange,
            makeListener: { _ in listener },
            openBrowser: { opened($0, $1, $2) },
            browserTimeout: 1)
    }

    private let input = ProvisionInput(email: "be@x.co", browser: .chrome, profilePath: "Profile 1")

    func testOpensTheProfileAndExchangesTheCallbackCode() async throws {
        let exchange = FakeExchange(); exchange.email = "be@x.co"
        let listener = FakeListener(); listener.code = "browser-be"
        var openedBrowser: Browser?; var openedProfile: String?; var openedURL: URL?
        let provisioner = makeProvisioner(exchange, listener: listener,
                                          opened: { openedBrowser = $0; openedProfile = $1; openedURL = $2 })

        var awaited = false
        let entry = try await provisioner.provision(input) { awaited = true }

        XCTAssertTrue(awaited, "onAwaitingBrowser fires once the browser opens")
        XCTAssertEqual(openedBrowser, .chrome)
        XCTAssertEqual(openedProfile, "Profile 1")
        XCTAssertEqual(exchange.codes, ["browser-be"])
        XCTAssertEqual(OAuthAccountJSON.email(entry.oauthAccount), "be@x.co")
        // The authorize URL and the exchange must share the loopback redirect.
        let q = try XCTUnwrap(URLComponents(url: try XCTUnwrap(openedURL), resolvingAgainstBaseURL: false)?.queryItems)
        let redirect = q.first { $0.name == "redirect_uri" }?.value
        XCTAssertEqual(redirect, "http://localhost:4321/callback")
        XCTAssertEqual(exchange.redirects, ["http://localhost:4321/callback"])
        XCTAssertEqual(q.first { $0.name == "login_hint" }?.value, "be@x.co")
    }

    func testRejectsAMintedLoginForAnotherAccount() async {
        let exchange = FakeExchange(); exchange.email = "someone.else@x.co"
        do {
            _ = try await makeProvisioner(exchange).provision(input) { }
            XCTFail("expected a mismatch throw")
        } catch {
            XCTAssertEqual(error as? ProvisionError, .emailMismatch(expected: "be@x.co", got: "someone.else@x.co"))
        }
    }

    func testEmailMatchIsCaseInsensitive() async throws {
        let exchange = FakeExchange(); exchange.email = "BE@X.CO"
        let entry = try await makeProvisioner(exchange).provision(input) { }
        XCTAssertEqual(OAuthAccountJSON.email(entry.oauthAccount), "BE@X.CO")
    }
}
