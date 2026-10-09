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
        // The newly-active account floats to the top immediately, without a refresh.
        XCTAssertEqual(vm.accountStates.first?.id, backend.id)

        // Dismissing the success alert asks the card lists to scroll to the top, once.
        XCTAssertEqual(vm.scrollToTopRequest, 0)
        vm.dismissSwitchMessage()
        XCTAssertNil(vm.switchMessage)
        XCTAssertEqual(vm.scrollToTopRequest, 1)
        vm.switchMessage = "Something else"
        vm.dismissSwitchMessage()
        XCTAssertEqual(vm.scrollToTopRequest, 1)
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

    func testActiveAccountHasQuotaLeftBelowBothThresholds() {
        func usage(_ fiveHour: Double, _ sevenDay: Double) -> UsageData {
            UsageData(fiveHour: UsageLimit(utilization: fiveHour, resetsAt: nil),
                      sevenDay: UsageLimit(utilization: sevenDay, resetsAt: nil))
        }
        XCTAssertTrue(DashboardViewModel.hasQuotaLeft(usage(94.9, 98.9)))
        XCTAssertFalse(DashboardViewModel.hasQuotaLeft(usage(95, 10)))
        XCTAssertFalse(DashboardViewModel.hasQuotaLeft(usage(10, 99)))
    }

    func testSwitchAsksFirstWhileTheActiveAccountHasQuotaLeft() async throws {
        let kc = InMemoryKeychain()
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let vault = KeychainCredentialVault(keychain: kc)
        let switcher = ClaudeCodeSwitcher(slot: slot, vault: vault, config: ClaudeConfigFile(fileURL: configURL),
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
        try JSONSerialization.data(withJSONObject: ["oauthAccount": ["emailAddress": "frontend@gotitapp.co"]]).write(to: configURL)
        try slot.overwrite(OAuthCredential(object: ["refreshToken": "f1", "expiresAt": soon, "refreshTokenExpiresAt": later])!)
        try vault.save(VaultEntry(
            oauth: OAuthCredential(object: ["refreshToken": "b1", "expiresAt": soon, "refreshTokenExpiresAt": later + 1000])!,
            oauthAccount: OAuthAccountJSON.canonical(["emailAddress": "backend@gotitapp.co"])!), for: backend.id)
        let vm = DashboardViewModel(accountStore: store,
                                    ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                                    ccSwitcher: switcher)
        await vm.refreshAll()
        let activeIndex = try XCTUnwrap(vm.accountStates.firstIndex { $0.id == frontend.id })
        vm.accountStates[activeIndex].usage = UsageData(fiveHour: UsageLimit(utilization: 40, resetsAt: nil),
                                                        sevenDay: UsageLimit(utilization: 60, resetsAt: nil))

        // Cancel leaves Claude Code where it was.
        await vm.requestSwitchClaudeCode(to: backend.id)
        XCTAssertEqual(vm.pendingSwitch?.targetId, backend.id)
        XCTAssertEqual(vm.pendingSwitch?.message.contains("frontend@gotitapp.co"), true)
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
        vm.cancelPendingSwitch()
        XCTAssertNil(vm.pendingSwitch)
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")

        // Confirm runs the switch.
        await vm.requestSwitchClaudeCode(to: backend.id)
        await vm.confirmPendingSwitch()
        XCTAssertNil(vm.pendingSwitch)
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
        XCTAssertEqual(vm.activeClaudeCodeEmail, "backend@gotitapp.co")

        // An exhausted active account switches straight away.
        let backendIndex = try XCTUnwrap(vm.accountStates.firstIndex { $0.id == backend.id })
        vm.accountStates[backendIndex].usage = UsageData(fiveHour: UsageLimit(utilization: 96, resetsAt: nil),
                                                         sevenDay: UsageLimit(utilization: 60, resetsAt: nil))
        vm.dismissSwitchMessage()
        await vm.requestSwitchClaudeCode(to: frontend.id)
        XCTAssertNil(vm.pendingSwitch)
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
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
