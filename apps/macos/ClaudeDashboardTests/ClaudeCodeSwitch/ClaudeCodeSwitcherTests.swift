import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeSwitcherTests: XCTestCase {

    private var kc: InMemoryKeychain!
    private var configURL: URL!
    private var now: Date!
    private var sleeps: [TimeInterval]!
    private var onSleep: (() -> Void)?

    private let frontend = ClaudeCodeSwitcherTests.account("frontend@gotitapp.co")
    private let backend = ClaudeCodeSwitcherTests.account("backend@gotitapp.co")

    override func setUp() {
        super.setUp()
        kc = InMemoryKeychain()
        configURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("ClaudeCodeSwitcherTests-\(UUID().uuidString).json")
        now = Date(timeIntervalSince1970: 1_760_000_000)
        sleeps = []
        onSleep = nil
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: configURL)
        super.tearDown()
    }

    // MARK: helpers

    private static func account(_ email: String?) -> Account {
        Account(id: UUID(), name: email ?? "no-email", email: email, chromeProfilePath: "",
                plan: .max5x, status: .active, source: .manual)
    }

    private var slot: KeychainClaudeCodeSlot { KeychainClaudeCodeSlot(keychain: kc, account: "me") }
    private var vault: KeychainCredentialVault { KeychainCredentialVault(keychain: kc) }

    private func makeSwitcher(config: ClaudeConfigAccountFile? = nil) -> ClaudeCodeSwitcher {
        ClaudeCodeSwitcher(
            slot: slot, vault: vault,
            config: config ?? ClaudeConfigFile(fileURL: configURL),
            now: { [unowned self] in self.now },
            sleep: { [unowned self] seconds in self.sleeps.append(seconds); self.onSleep?() })
    }

    private func cred(_ refresh: String, expiresIn: TimeInterval = 8 * 3600,
                      refreshExpiresIn: TimeInterval = 20 * 86400) -> OAuthCredential {
        let ms = { (t: TimeInterval) in Int((self.now.timeIntervalSince1970 + t) * 1000) }
        return OAuthCredential(object: ["accessToken": "a-\(refresh)", "refreshToken": refresh,
                                        "expiresAt": ms(expiresIn), "refreshTokenExpiresAt": ms(refreshExpiresIn)])!
    }

    private func accountJSON(_ email: String) -> Data {
        OAuthAccountJSON.canonical(["emailAddress": email, "organizationName": "MathGPT.ai"])!
    }

    /// Claude Code is signed in as `email` with `credential`.
    private func signIn(_ email: String, _ credential: OAuthCredential) throws {
        try JSONSerialization.data(withJSONObject: ["oauthAccount": OAuthAccountJSON.object(accountJSON(email))!, "projects": [:]])
            .write(to: configURL)
        try slot.writeOAuth(credential)
    }

    // MARK: capture

    func testCaptureSavesActiveCredentialToMatchingAccount() throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend, backend]), .saved(frontend.id))
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend, backend]), .unchanged(frontend.id))
    }

    func testCaptureMatchesEmailCaseInsensitively() throws {
        try signIn("Frontend@GotItApp.co", cred("f1"))
        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend]), .saved(frontend.id))
    }

    func testCaptureIgnoresAccountWithoutEmail() throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        XCTAssertEqual(try makeSwitcher().capture(accounts: [Self.account(nil)]), .noActiveAccount)
    }

    func testCaptureNeverOverwritesVaultWithBlankedCredential() throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        _ = try makeSwitcher().capture(accounts: [frontend])
        try slot.writeOAuth(OAuthCredential(object: ["accessToken": "", "refreshToken": "", "expiresAt": 0])!)

        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend]), .blanked(frontend.id))
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
    }

    // MARK: availability

    func testAvailability() throws {
        let expired = Self.account("old@gotitapp.co")
        let blank = Self.account("blank@gotitapp.co")
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try vault.save(VaultEntry(oauth: cred("o1", refreshExpiresIn: -1), oauthAccount: accountJSON("old@gotitapp.co")), for: expired.id)
        try signIn("frontend@gotitapp.co", cred("f1"))
        let unknown = Self.account("new@gotitapp.co")

        let map = makeSwitcher().availability(for: [frontend, backend, expired, unknown])
        XCTAssertEqual(map[frontend.id], .active)
        XCTAssertEqual(map[backend.id], .ready)
        XCTAssertEqual(map[expired.id], .loginExpired)
        XCTAssertEqual(map[unknown.id], .notCaptured)

        try signIn("blank@gotitapp.co", OAuthCredential(object: ["refreshToken": ""])!)
        XCTAssertEqual(makeSwitcher().availability(for: [blank])[blank.id], .needsLogin)
    }

    // MARK: switch

    func testSwitchSavesCurrentThenActivatesTargetKeepingMcpOAuth() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        let root = try XCTUnwrap(OAuthAccountJSON.object(try XCTUnwrap(kc.items["Claude Code-credentials|me"])))
        var withMcp = root; withMcp["mcpOAuth"] = ["figma": ["accessToken": "keep"]]
        kc.items["Claude Code-credentials|me"] = try JSONSerialization.data(withJSONObject: withMcp)
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        // Claude Code rotated frontend's token after the last capture: the switch must save f2, not f1.
        try slot.writeOAuth(cred("f2"))

        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])

        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(ClaudeConfigFile(fileURL: configURL).readOAuthAccount())), "backend@gotitapp.co")
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f2")
        let after = try XCTUnwrap(OAuthAccountJSON.object(try XCTUnwrap(kc.items["Claude Code-credentials|me"])))
        XCTAssertNotNil(after["mcpOAuth"])
        XCTAssertEqual(sleeps, [])
    }

    func testSwitchRefusesUncapturedOrExpiredTarget() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        do { try await makeSwitcher().switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .notCaptured) }

        try vault.save(VaultEntry(oauth: cred("b1", refreshExpiresIn: -1), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        do { try await makeSwitcher().switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .loginExpired) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
    }

    func testSwitchRefusesWhenActiveAccountIsNotInDashboard() async throws {
        try signIn("stranger@gotitapp.co", cred("s1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        do { try await makeSwitcher().switchTo(backend, accounts: [backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .activeAccountNotInDashboard(email: "stranger@gotitapp.co")) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "s1")
    }

    func testSwitchToActiveAccountIsANoOp() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try await makeSwitcher().switchTo(frontend, accounts: [frontend])
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
    }

    func testSwitchWaitsForImminentRefreshThenSavesRotatedToken() async throws {
        try signIn("frontend@gotitapp.co", cred("f1", expiresIn: 120))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        var polls = 0
        onSleep = { [unowned self] in
            polls += 1
            if polls == 2 { try? self.slot.writeOAuth(self.cred("f2")) }   // a session refreshed
        }

        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])

        XCTAssertEqual(sleeps, [3, 3])
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f2")
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
    }

    func testSwitchContinuesAfterWaitTimesOut() async throws {
        try signIn("frontend@gotitapp.co", cred("f1", expiresIn: 120))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])
        XCTAssertEqual(sleeps.count, 10)
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
    }

    func testSwitchDoesNotWaitWhenTokenExpiredLongAgo() async throws {
        try signIn("frontend@gotitapp.co", cred("f1", expiresIn: -3 * 3600))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])
        XCTAssertEqual(sleeps, [])
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
    }

    func testSwitchRollsBackCredentialWhenConfigWriteFails() async throws {
        struct FailingConfig: ClaudeConfigAccountFile {
            let inner: ClaudeConfigFile
            func readOAuthAccount() throws -> Data? { try inner.readOAuthAccount() }
            func writeOAuthAccount(_ json: Data) throws { throw ClaudeConfigFileError.unreadable }
        }
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        let switcher = makeSwitcher(config: FailingConfig(inner: ClaudeConfigFile(fileURL: configURL)))

        do { try await switcher.switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .config) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
    }

    func testSwitchReportsKeychainFailureAndChangesNothing() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        _ = try makeSwitcher().capture(accounts: [frontend])
        kc.failWrites = true

        do { try await makeSwitcher().switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .keychain) }
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(ClaudeConfigFile(fileURL: configURL).readOAuthAccount())), "frontend@gotitapp.co")
    }

    func testLiveIsNilUnderTests() {
        XCTAssertNil(ClaudeCodeSwitcher.live(isRunningTests: true))
    }
}
