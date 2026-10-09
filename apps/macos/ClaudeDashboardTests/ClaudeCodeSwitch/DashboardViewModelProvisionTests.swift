import XCTest
@testable import ClaudeDashboard

/// Drives the view model's Switch flow for accounts with no vault copy: silent capture on
/// refresh, the browser fallback on a Switch tap, and the failure paths.
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

    private final class FakeListener: OAuthCallbackListening {
        var code = "browser-code"
        var onWait: (() -> Void)?
        func start() throws -> Int { 4321 }
        func waitForCode(timeout: TimeInterval) async throws -> String { onWait?(); return code }
        func cancel() {}
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

    private func makeProvisioner(grant: FakeGrant, exchange: FakeExchange,
                                 listener: FakeListener, opened: @escaping (URL) -> Void = { _ in })
        -> ClaudeCodeLoginProvisioner {
        ClaudeCodeLoginProvisioner(
            grantClient: grant, oauthClient: exchange,
            makeListener: { _ in listener },
            openBrowser: { _, _, url in opened(url) },
            browserTimeout: 1)
    }

    private func makeViewModel(store: AccountStore, switcher: ClaudeCodeSwitcher,
                               provisioner: ClaudeCodeLoginProvisioner) -> DashboardViewModel {
        DashboardViewModel(accountStore: store,
                           ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                           ccSwitcher: switcher, ccProvisioner: provisioner)
    }

    // MARK: Tests

    func testRefreshSilentlyCapturesAFreshAccountThenSwitchJustWorks() async throws {
        let kc = InMemoryKeychain()
        let (store, frontend, backend) = try makeStore()
        let switcher = try makeSwitcher(kc, frontend: frontend)
        var opened = false
        let provisioner = makeProvisioner(grant: FakeGrant(.code("silent-be")),
                                          exchange: FakeExchange(email: backend.email!),
                                          listener: FakeListener(), opened: { _ in opened = true })
        let vm = makeViewModel(store: store, switcher: switcher, provisioner: provisioner)

        await vm.refreshAll()
        XCTAssertEqual(vm.switchAvailability[backend.id], .ready, "a fresh session is captured silently")
        XCTAssertFalse(opened, "silent capture never opens a browser")

        await vm.switchClaudeCode(to: backend.id)
        XCTAssertEqual(try KeychainClaudeCodeSlot(keychain: kc, account: "me").readOAuth()?.refreshToken, "rt-silent-be")
        XCTAssertEqual(vm.switchAvailability[backend.id], .active)
    }

    func testSwitchOnStaleAccountGoesThroughTheBrowser() async throws {
        let kc = InMemoryKeychain()
        let (store, frontend, backend) = try makeStore()
        let switcher = try makeSwitcher(kc, frontend: frontend)
        let listener = FakeListener(); listener.code = "browser-be"
        var openedURL: URL?
        let provisioner = makeProvisioner(grant: FakeGrant(.stale),
                                          exchange: FakeExchange(email: backend.email!),
                                          listener: listener, opened: { openedURL = $0 })
        let vm = makeViewModel(store: store, switcher: switcher, provisioner: provisioner)

        await vm.refreshAll()
        XCTAssertEqual(vm.switchAvailability[backend.id], .notCaptured, "a stale session is not captured silently")

        var awaitingDuringWait: UUID?
        listener.onWait = { awaitingDuringWait = vm.awaitingBrowserAccount }
        await vm.switchClaudeCode(to: backend.id)

        XCTAssertNotNil(openedURL, "a stale Switch opens the browser")
        XCTAssertEqual(awaitingDuringWait, backend.id, "the UI shows the waiting state while the browser is open")
        XCTAssertNil(vm.awaitingBrowserAccount, "the waiting state clears when done")
        XCTAssertEqual(try KeychainClaudeCodeSlot(keychain: kc, account: "me").readOAuth()?.refreshToken, "rt-browser-be")
        XCTAssertEqual(vm.switchAvailability[backend.id], .active)
    }

    func testSwitchWithoutASessionKeyAsksForResync() async throws {
        let kc = InMemoryKeychain()
        let store = AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        let frontend = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "Default",
                               orgId: "org-fe", sessionKey: "sk-fe", browser: .chrome,
                               plan: .max5x, status: .active, source: .browser)
        // No sessionKey, no org: a manual record the dashboard cannot provision.
        let manual = Account(id: UUID(), name: "manual", email: "manual@x.co", chromeProfilePath: "",
                             plan: .max5x, status: .active, source: .manual)
        store.addAccount(frontend)
        store.addAccount(manual)
        let switcher = try makeSwitcher(kc, frontend: frontend)
        let provisioner = makeProvisioner(grant: FakeGrant(.code("x")),
                                          exchange: FakeExchange(email: manual.email!), listener: FakeListener())
        let vm = makeViewModel(store: store, switcher: switcher, provisioner: provisioner)

        await vm.refreshAll()
        await vm.switchClaudeCode(to: manual.id)
        XCTAssertEqual(vm.switchMessage?.contains("Re-sync"), true)
        XCTAssertNil(try KeychainCredentialVault(keychain: kc).load(manual.id), "nothing was provisioned")
    }

    func testBrowserSignInAsWrongAccountIsRejected() async throws {
        let kc = InMemoryKeychain()
        let (store, frontend, backend) = try makeStore()
        let switcher = try makeSwitcher(kc, frontend: frontend)
        // Silent returns a code, but the minted login is someone else: refuse and switch nothing.
        let provisioner = makeProvisioner(grant: FakeGrant(.code("c")),
                                          exchange: FakeExchange(email: "intruder@x.co"), listener: FakeListener())
        let vm = makeViewModel(store: store, switcher: switcher, provisioner: provisioner)

        await vm.refreshAll()
        // Silent capture on refresh rejected the mismatch, so the account stays uncaptured.
        XCTAssertEqual(vm.switchAvailability[backend.id], .notCaptured)

        await vm.switchClaudeCode(to: backend.id)
        XCTAssertEqual(vm.switchMessage?.contains("intruder@x.co"), true)
        XCTAssertEqual(try KeychainClaudeCodeSlot(keychain: kc, account: "me").readOAuth()?.refreshToken, "fe",
                       "Claude Code still signed in as frontend")
    }
}
