import XCTest
@testable import ClaudeDashboard

/// Drives the view model's Switch flow for accounts with no vault copy: silent capture on
/// refresh and on a Switch tap, plus the "sign in first" and mismatch failure paths.
@MainActor
final class DashboardViewModelProvisionTests: XCTestCase {

    private var suite: String!
    private var configURL: URL!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        configURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("DashboardViewModelProvisionTests-\(UUID().uuidString).json")
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(at: configURL)
        super.tearDown()
    }

    // MARK: Fakes

    private final class FakeGrant: ClaudeAIGrantRequesting {
        var result: GrantResult
        init(_ result: GrantResult) { self.result = result }
        func grantCode(orgId: String, sessionKey: String, pkce: PKCE,
                       redirectURI: String, loginHint: String?) async throws -> GrantResult { result }
    }

    private final class FakeExchange: ClaudeCodeTokenExchanging {
        var email: String
        init(email: String) { self.email = email }
        func exchange(code: String, pkce: PKCE, redirectURI: String) async throws -> VaultEntry {
            let future = Int((Date().timeIntervalSince1970 + 30 * 86400) * 1000)
            return VaultEntry(
                oauth: OAuthCredential(object: ["refreshToken": "rt-\(code)", "refreshTokenExpiresAt": future])!,
                oauthAccount: OAuthAccountJSON.canonical(["emailAddress": email])!)
        }
    }

    // MARK: Helpers

    private func makeStore() throws -> (AccountStore, Account, Account) {
        let store = AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        let frontend = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "Default",
                               orgId: "org-fe", sessionKey: "sk-fe", browser: .chrome,
                               plan: .max5x, status: .active, source: .browser)
        let backend = Account(id: UUID(), name: "be", email: "backend@gotitapp.co", chromeProfilePath: "Profile 1",
                              orgId: "org-be", sessionKey: "sk-be", browser: .chrome,
                              plan: .max5x, status: .active, source: .browser)
        store.addAccount(frontend)
        store.addAccount(backend)
        return (store, frontend, backend)
    }

    /// A switcher whose Claude Code is signed in as `frontend` with a live credential.
    private func makeSwitcher(_ kc: InMemoryKeychain, frontend: Account) throws -> ClaudeCodeSwitcher {
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let future = Int((Date().timeIntervalSince1970 + 7 * 3600) * 1000)
        let later = Int((Date().timeIntervalSince1970 + 30 * 86400) * 1000)
        try slot.overwrite(OAuthCredential(object: ["refreshToken": "fe", "expiresAt": future,
                                                    "refreshTokenExpiresAt": later])!)
        try JSONSerialization.data(withJSONObject: ["oauthAccount": ["emailAddress": frontend.email!]])
            .write(to: configURL)
        return ClaudeCodeSwitcher(slot: slot, vault: KeychainCredentialVault(keychain: kc),
                                  config: ClaudeConfigFile(fileURL: configURL), now: Date.init, sleep: { _ in })
    }

    private func makeViewModel(store: AccountStore, switcher: ClaudeCodeSwitcher,
                               grant: FakeGrant, exchange: FakeExchange) -> DashboardViewModel {
        DashboardViewModel(accountStore: store,
                           ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                           ccSwitcher: switcher,
                           ccProvisioner: ClaudeCodeLoginProvisioner(grantClient: grant, oauthClient: exchange))
    }

    private func slot(_ kc: InMemoryKeychain) -> KeychainClaudeCodeSlot {
        KeychainClaudeCodeSlot(keychain: kc, account: "me")
    }

    // MARK: Tests

    func testRefreshSilentlyCapturesAFreshAccountThenSwitchJustWorks() async throws {
        let kc = InMemoryKeychain()
        let (store, frontend, backend) = try makeStore()
        let switcher = try makeSwitcher(kc, frontend: frontend)
        let vm = makeViewModel(store: store, switcher: switcher,
                               grant: FakeGrant(.code("silent-be")), exchange: FakeExchange(email: backend.email!))

        await vm.refreshAll()
        XCTAssertEqual(vm.switchAvailability[backend.id], .ready, "a fresh session is captured silently")

        await vm.switchClaudeCode(to: backend.id)
        XCTAssertEqual(try slot(kc).readOAuth()?.refreshToken, "rt-silent-be")
        XCTAssertEqual(vm.switchAvailability[backend.id], .active)
    }

    func testSwitchOnStaleAccountAsksTheUserToSignIn() async throws {
        let kc = InMemoryKeychain()
        let (store, frontend, backend) = try makeStore()
        let switcher = try makeSwitcher(kc, frontend: frontend)
        let vm = makeViewModel(store: store, switcher: switcher,
                               grant: FakeGrant(.stale), exchange: FakeExchange(email: backend.email!))

        await vm.refreshAll()
        XCTAssertEqual(vm.switchAvailability[backend.id], .notCaptured, "a stale session is not captured")

        await vm.switchClaudeCode(to: backend.id)
        XCTAssertEqual(vm.switchMessage?.contains("Sign in to claude.ai"), true)
        XCTAssertEqual(vm.switchMessage?.contains("backend@gotitapp.co"), true)
        XCTAssertEqual(try slot(kc).readOAuth()?.refreshToken, "fe", "nothing switched")
        XCTAssertEqual(vm.switchAvailability[backend.id], .notCaptured)
    }

    func testSwitchWithoutASessionKeyAsksForResync() async throws {
        let kc = InMemoryKeychain()
        let store = AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        let frontend = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "Default",
                               orgId: "org-fe", sessionKey: "sk-fe", browser: .chrome,
                               plan: .max5x, status: .active, source: .browser)
        let manual = Account(id: UUID(), name: "manual", email: "manual@x.co", chromeProfilePath: "",
                             plan: .max5x, status: .active, source: .manual)
        store.addAccount(frontend)
        store.addAccount(manual)
        let switcher = try makeSwitcher(kc, frontend: frontend)
        let vm = makeViewModel(store: store, switcher: switcher,
                               grant: FakeGrant(.code("x")), exchange: FakeExchange(email: manual.email!))

        await vm.refreshAll()
        await vm.switchClaudeCode(to: manual.id)
        XCTAssertEqual(vm.switchMessage?.contains("Re-sync"), true)
        XCTAssertNil(try KeychainCredentialVault(keychain: kc).load(manual.id), "nothing was provisioned")
    }

    func testSilentCaptureAsAnotherAccountIsRejected() async throws {
        let kc = InMemoryKeychain()
        let (store, frontend, backend) = try makeStore()
        let switcher = try makeSwitcher(kc, frontend: frontend)
        // Silent returns a code, but the minted login is someone else: refuse, capture nothing.
        let vm = makeViewModel(store: store, switcher: switcher,
                               grant: FakeGrant(.code("c")), exchange: FakeExchange(email: "intruder@x.co"))

        await vm.refreshAll()
        XCTAssertEqual(vm.switchAvailability[backend.id], .notCaptured)

        await vm.switchClaudeCode(to: backend.id)
        XCTAssertEqual(vm.switchMessage?.contains("intruder@x.co"), true)
        XCTAssertEqual(try slot(kc).readOAuth()?.refreshToken, "fe", "Claude Code still signed in as frontend")
    }
}
