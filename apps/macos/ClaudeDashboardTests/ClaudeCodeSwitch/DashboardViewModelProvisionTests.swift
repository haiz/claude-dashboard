import XCTest
@testable import ClaudeDashboard

/// Drives the view model's Switch flow for accounts with no vault copy: the browser-profile
/// provision, then the switch, plus the "needs a browser profile" and mismatch failures.
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

    private func makeViewModel(store: AccountStore, switcher: ClaudeCodeSwitcher,
                              exchange: FakeExchange, listener: FakeListener,
                              opened: @escaping (URL) -> Void = { _ in }) -> DashboardViewModel {
        let provisioner = ClaudeCodeLoginProvisioner(
            oauthClient: exchange, makeListener: { _ in listener },
            openBrowser: { _, _, url in opened(url) }, browserTimeout: 1)
        return DashboardViewModel(accountStore: store,
                                  ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                                  ccSwitcher: switcher, ccProvisioner: provisioner)
    }

    private func slot(_ kc: InMemoryKeychain) -> KeychainClaudeCodeSlot {
        KeychainClaudeCodeSlot(keychain: kc, account: "me")
    }

    // MARK: Tests

    func testSwitchOnUncapturedProvisionsViaBrowserThenSwitches() async throws {
        let kc = InMemoryKeychain()
        let (store, frontend, backend) = try makeStore()
        let switcher = try makeSwitcher(kc, frontend: frontend)
        let listener = FakeListener(); listener.code = "browser-be"
        var openedURL: URL?
        let vm = makeViewModel(store: store, switcher: switcher,
                               exchange: FakeExchange(email: backend.email!), listener: listener,
                               opened: { openedURL = $0 })

        await vm.refreshAll()
        XCTAssertEqual(vm.switchAvailability[backend.id], .notCaptured)

        var awaitingDuringWait: UUID?
        listener.onWait = { awaitingDuringWait = vm.awaitingBrowserAccount }
        await vm.switchClaudeCode(to: backend.id)

        XCTAssertNotNil(openedURL, "the account's browser profile is opened on the consent page")
        XCTAssertEqual(awaitingDuringWait, backend.id, "the UI shows the waiting state while the browser is open")
        XCTAssertNil(vm.awaitingBrowserAccount, "the waiting state clears when done")
        XCTAssertEqual(try slot(kc).readOAuth()?.refreshToken, "rt-browser-be")
        XCTAssertEqual(vm.switchAvailability[backend.id], .active)
    }

    func testSwitchWithoutABrowserProfileAsksForResync() async throws {
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
        var opened = false
        let vm = makeViewModel(store: store, switcher: switcher,
                               exchange: FakeExchange(email: manual.email!), listener: FakeListener(),
                               opened: { _ in opened = true })

        await vm.refreshAll()
        await vm.switchClaudeCode(to: manual.id)
        XCTAssertEqual(vm.switchMessage?.contains("Re-sync"), true)
        XCTAssertFalse(opened, "no browser opens for a record with no profile")
        XCTAssertNil(try KeychainCredentialVault(keychain: kc).load(manual.id))
    }

    func testBrowserSignInAsWrongAccountIsRejected() async throws {
        let kc = InMemoryKeychain()
        let (store, frontend, backend) = try makeStore()
        let switcher = try makeSwitcher(kc, frontend: frontend)
        let vm = makeViewModel(store: store, switcher: switcher,
                               exchange: FakeExchange(email: "intruder@x.co"), listener: FakeListener())

        await vm.refreshAll()
        await vm.switchClaudeCode(to: backend.id)
        XCTAssertEqual(vm.switchMessage?.contains("intruder@x.co"), true)
        XCTAssertEqual(try slot(kc).readOAuth()?.refreshToken, "fe", "Claude Code still signed in as frontend")
    }
}
