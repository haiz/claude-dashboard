import XCTest
@testable import ClaudeDashboard

@MainActor
final class DashboardViewModelSwitchTests: XCTestCase {

    private var suite: String!
    private var configURL: URL!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        configURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("DashboardViewModelSwitchTests-\(UUID().uuidString).json")
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(at: configURL)
        super.tearDown()
    }

    func testRefreshPublishesAvailabilityAndSwitchMovesClaudeCode() async throws {
        let kc = InMemoryKeychain()
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let vault = KeychainCredentialVault(keychain: kc)
        let config = ClaudeConfigFile(fileURL: configURL)
        let switcher = ClaudeCodeSwitcher(slot: slot, vault: vault, config: config,
                                          now: Date.init, sleep: { _ in })
        let store = AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        let frontend = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "",
                               plan: .max5x, status: .active, source: .manual)
        let backend = Account(id: UUID(), name: "be", email: "backend@gotitapp.co", chromeProfilePath: "",
                              plan: .max5x, status: .active, source: .manual)
        store.addAccount(frontend)
        store.addAccount(backend)
        let soon = Int((Date().timeIntervalSince1970 + 8 * 3600) * 1000)
        let later = Int((Date().timeIntervalSince1970 + 20 * 86400) * 1000)
        // Each grant has its own fixed deadline (spec fact 4); the switcher tells accounts apart by it.
        let backendLater = later + 1000
        try JSONSerialization.data(withJSONObject: ["oauthAccount": ["emailAddress": "frontend@gotitapp.co"]]).write(to: configURL)
        try slot.overwrite(OAuthCredential(object: ["refreshToken": "f1", "expiresAt": soon, "refreshTokenExpiresAt": later])!)
        try vault.save(VaultEntry(
            oauth: OAuthCredential(object: ["refreshToken": "b1", "expiresAt": soon, "refreshTokenExpiresAt": backendLater])!,
            oauthAccount: OAuthAccountJSON.canonical(["emailAddress": "backend@gotitapp.co"])!), for: backend.id)

        let vm = DashboardViewModel(accountStore: store,
                                    ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                                    ccSwitcher: switcher)
        await vm.refreshAll()

        XCTAssertEqual(vm.switchAvailability[frontend.id], .active)
        XCTAssertEqual(vm.switchAvailability[backend.id], .ready)

        await vm.switchClaudeCode(to: backend.id)

        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
        XCTAssertEqual(vm.activeClaudeCodeEmail, "backend@gotitapp.co")
        XCTAssertEqual(vm.switchAvailability[backend.id], .active)
        XCTAssertEqual(vm.switchAvailability[frontend.id], .ready)
        XCTAssertEqual(vm.switchMessage?.contains("backend@gotitapp.co"), true)
        XCTAssertFalse(vm.isSwitchingClaudeCode)
    }

    func testRemovingAnAccountDeletesItsVaultCopy() async throws {
        let kc = InMemoryKeychain()
        let vault = KeychainCredentialVault(keychain: kc)
        let switcher = ClaudeCodeSwitcher(slot: KeychainClaudeCodeSlot(keychain: kc, account: "me"), vault: vault,
                                          config: ClaudeConfigFile(fileURL: configURL), now: Date.init, sleep: { _ in })
        let store = AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        let kept = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "",
                           plan: .max5x, status: .active, source: .manual)
        let removed = Account(id: UUID(), name: "be", email: "backend@gotitapp.co", chromeProfilePath: "",
                              plan: .max5x, status: .active, source: .manual)
        store.addAccount(kept)
        store.addAccount(removed)
        for account in [kept, removed] {
            try vault.save(VaultEntry(
                oauth: OAuthCredential(object: ["refreshToken": "r-\(account.name)"])!,
                oauthAccount: OAuthAccountJSON.canonical(["emailAddress": account.email!])!), for: account.id)
        }
        let vm = DashboardViewModel(accountStore: store,
                                    ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                                    ccSwitcher: switcher)
        // AccountStore.$accounts reaches the view model via .receive(on: .main).
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 2)

        store.removeAccount(id: removed.id)

        // The delete runs on a detached task: poll, bounded.
        for _ in 0..<200 where try vault.load(removed.id) != nil {
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertNil(try vault.load(removed.id))
        XCTAssertNotNil(try vault.load(kept.id))
    }

    func testErrorMessagesNameTheFix() {
        let a = Account(id: UUID(), name: "be", email: "backend@gotitapp.co", chromeProfilePath: "",
                        plan: .max5x, status: .active, source: .manual)
        XCTAssertTrue(DashboardViewModel.message(for: .notCaptured, target: a).contains("/login"))
        XCTAssertTrue(DashboardViewModel.message(for: .activeAccountNotInDashboard(email: "x@y.z"), target: a).contains("x@y.z"))
        XCTAssertTrue(DashboardViewModel.message(for: .activeAccountUnknown, target: a).contains("/login"))
        XCTAssertTrue(DashboardViewModel.message(for: .rollbackFailed, target: a).contains("/login"))
    }
}
