import XCTest
@testable import ClaudeDashboard

@MainActor
final class DashboardViewModelAutoSwitchTests: XCTestCase {

    private var suite: String!
    private var configURL: URL!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        configURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("DashboardViewModelAutoSwitchTests-\(UUID().uuidString).json")
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(at: configURL)
        super.tearDown()
    }

    private struct Harness {
        let vm: DashboardViewModel
        let slot: KeychainClaudeCodeSlot
        let frontend: Account
        let backend: Account
        let settings: AutoSwitchSettings
    }

    /// frontend is active in Claude Code; backend has a vault copy only when `captureBackend`.
    private func makeHarness(captureBackend: Bool) async throws -> Harness {
        let kc = InMemoryKeychain()
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let vault = KeychainCredentialVault(keychain: kc)
        let switcher = ClaudeCodeSwitcher(slot: slot, vault: vault, config: ClaudeConfigFile(fileURL: configURL),
                                          now: Date.init, sleep: { _ in })
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        let store = AccountStore(defaults: defaults)
        let frontend = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "",
                               plan: .max5x, status: .active, source: .manual)
        let backend = Account(id: UUID(), name: "be", email: "backend@gotitapp.co", chromeProfilePath: "",
                              plan: .max5x, status: .active, source: .manual)
        store.addAccount(frontend)
        store.addAccount(backend)
        let soon = Int((Date().timeIntervalSince1970 + 8 * 3600) * 1000)
        let later = Int((Date().timeIntervalSince1970 + 20 * 86400) * 1000)
        try JSONSerialization.data(withJSONObject: ["oauthAccount": ["emailAddress": "frontend@gotitapp.co"]])
            .write(to: configURL)
        try slot.overwrite(OAuthCredential(object: ["refreshToken": "f1", "expiresAt": soon,
                                                    "refreshTokenExpiresAt": later])!)
        if captureBackend {
            try vault.save(VaultEntry(
                oauth: OAuthCredential(object: ["refreshToken": "b1", "expiresAt": soon,
                                                "refreshTokenExpiresAt": later + 1000])!,
                oauthAccount: OAuthAccountJSON.canonical(["emailAddress": "backend@gotitapp.co"])!), for: backend.id)
        }
        let settings = AutoSwitchSettings(defaults: defaults)
        let vm = DashboardViewModel(accountStore: store,
                                    ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                                    ccSwitcher: switcher, autoSwitchSettings: settings)
        await vm.refreshAll()
        return Harness(vm: vm, slot: slot, frontend: frontend, backend: backend, settings: settings)
    }

    private func setUsage(_ vm: DashboardViewModel, _ id: UUID, five: Double, seven: Double) throws {
        let index = try XCTUnwrap(vm.accountStates.firstIndex { $0.id == id })
        vm.accountStates[index].usage = UsageData(
            fiveHour: UsageLimit(utilization: five, resetsAt: Date().addingTimeInterval(3600)),
            sevenDay: UsageLimit(utilization: seven, resetsAt: Date().addingTimeInterval(86_400)))
    }

    func testAutoSwitchMovesClaudeCodeWithoutAnAlert() async throws {
        let h = try await makeHarness(captureBackend: true)
        try setUsage(h.vm, h.frontend.id, five: 96, seven: 40)
        try setUsage(h.vm, h.backend.id, five: 10, seven: 20)
        h.settings.isEnabled = true

        await h.vm.runAutoSwitch()

        XCTAssertEqual(try h.slot.readOAuth()?.refreshToken, "b1")
        XCTAssertEqual(h.vm.activeClaudeCodeEmail, "backend@gotitapp.co")
        XCTAssertNil(h.vm.switchMessage, "auto switch reports in the footer, never as an alert")
        XCTAssertEqual(h.vm.autoSwitch.lastEvent?.kind, .switched(.threshold(tier: 1)))
        XCTAssertEqual(h.vm.accountStates.first?.id, h.backend.id)
        XCTAssertFalse(h.vm.isSwitchingClaudeCode)
    }

    func testAutoSwitchDoesNothingWhileDisabled() async throws {
        let h = try await makeHarness(captureBackend: true)
        try setUsage(h.vm, h.frontend.id, five: 99, seven: 40)
        try setUsage(h.vm, h.backend.id, five: 10, seven: 20)

        await h.vm.runAutoSwitch()

        XCTAssertEqual(try h.slot.readOAuth()?.refreshToken, "f1")
        XCTAssertNil(h.vm.autoSwitch.lastEvent)
    }

    func testAutoSwitchNeverProvisionsAnUncapturedAccount() async throws {
        let h = try await makeHarness(captureBackend: false)
        XCTAssertEqual(h.vm.switchAvailability[h.backend.id], .notCaptured)
        try setUsage(h.vm, h.frontend.id, five: 96, seven: 40)
        try setUsage(h.vm, h.backend.id, five: 10, seven: 20)
        h.settings.isEnabled = true

        await h.vm.runAutoSwitch()

        XCTAssertEqual(try h.slot.readOAuth()?.refreshToken, "f1")
        XCTAssertNil(h.vm.awaitingBrowserAccount)
        XCTAssertEqual(h.vm.autoSwitch.lastEvent?.kind, .noCandidate)
    }

    func testPollRunsNearALimitAndStopsWhenDisabled() async throws {
        let h = try await makeHarness(captureBackend: false)   // no candidate: nothing switches
        try setUsage(h.vm, h.frontend.id, five: 93, seven: 40)
        h.settings.isEnabled = true

        await h.vm.runAutoSwitch()
        XCTAssertNotNil(h.vm.autoSwitchPollTask, "in the warning zone the 60 s poll runs")

        h.settings.isEnabled = false
        h.vm.updateAutoSwitchPoll()
        XCTAssertNil(h.vm.autoSwitchPollTask)

        h.settings.isEnabled = true
        h.vm.updateAutoSwitchPoll()
        XCTAssertNotNil(h.vm.autoSwitchPollTask, "the poll restarts after it was stopped")
        h.settings.isEnabled = false
        h.vm.updateAutoSwitchPoll()
    }

    func testUnknownClaudeCodeLoginStaysPut() async throws {
        let h = try await makeHarness(captureBackend: true)
        try setUsage(h.vm, h.frontend.id, five: 99, seven: 40)
        try setUsage(h.vm, h.backend.id, five: 10, seven: 20)
        h.vm.activeClaudeCodeEmail = "stranger@example.com"
        h.settings.isEnabled = true

        await h.vm.runAutoSwitch()

        XCTAssertEqual(try h.slot.readOAuth()?.refreshToken, "f1")
        XCTAssertNil(h.vm.autoSwitch.lastEvent)
        XCTAssertNil(h.vm.autoSwitchPollTask)
    }

    func testTickRecordsTheFiveHourBaselineAndToggles() async throws {
        let h = try await makeHarness(captureBackend: true)
        try setUsage(h.vm, h.backend.id, five: 50, seven: 20)
        let state = try XCTUnwrap(h.vm.accountStates.first { $0.id == h.backend.id })
        let active = try XCTUnwrap(h.vm.accountStates.first { $0.id == h.frontend.id })
        XCTAssertTrue(h.vm.canTickPriority(state))
        XCTAssertFalse(h.vm.canTickPriority(active), "the active account cannot be ticked")

        h.vm.togglePriority(for: h.backend.id)
        XCTAssertEqual(h.settings.priority?.accountId, h.backend.id)
        XCTAssertEqual(h.settings.priority?.baseline, state.usage?.fiveHour.resetsAt)
        XCTAssertTrue(h.vm.isPriorityTarget(h.backend.id))

        h.vm.togglePriority(for: h.backend.id)
        XCTAssertNil(h.settings.priority)
    }
}
