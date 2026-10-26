@testable import ClaudeDashboard
import XCTest

private final class FakeConfig: ClaudeDesktopConfigReadingWriting {
    var grant: (accountUuid: String, oauth: OAuthCredential)?
    var writes: [String] = []
    func readActiveGrant() throws -> (accountUuid: String, oauth: OAuthCredential)? { grant }
    func writeGrant(_ oauth: OAuthCredential, accountUuid: String, orgUuid: String) throws {
        writes.append(accountUuid)
        grant = (accountUuid, oauth)
    }
}

private final class FakeVault: CredentialVaulting {
    var items: [UUID: VaultEntry] = [:]
    func load(_ accountId: UUID) throws -> VaultEntry? { items[accountId] }
    func save(_ entry: VaultEntry, for accountId: UUID) throws { items[accountId] = entry }
    func delete(_ accountId: UUID) throws { items[accountId] = nil }
}

private final class FakeProcess: ClaudeDesktopProcessControlling {
    var isRunning = false
    var quitCalls = 0
    func quit() -> Bool { quitCalls += 1; isRunning = false; return true }
}

/// The manual Switch with Claude Desktop: the checkbox, the close confirmation, the look-ahead
/// write, and the detail-page button. Claude Code is on `frontend`, Desktop too; `backend` is
/// captured in both vaults.
@MainActor
final class DesktopSwitchFlowTests: XCTestCase {
    private var suite: String!
    private var configURL: URL!
    private let desktopConfig = FakeConfig()
    private let desktopVault = FakeVault()
    private let process = FakeProcess()
    private var vm: DashboardViewModel!
    private var frontend: Account!
    private var backend: Account!

    override func setUp() async throws {
        suite = StoreFixture.makeSuiteName()
        configURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("DesktopSwitchFlowTests-\(UUID().uuidString).json")
        AppDefaults.shared.removeObject(forKey: AppDefaults.switchDesktopEnabledKey)

        let kc = InMemoryKeychain()
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let ccVault = KeychainCredentialVault(keychain: kc)
        let ccSwitcher = ClaudeCodeSwitcher(slot: slot, vault: ccVault, config: ClaudeConfigFile(fileURL: configURL),
                                            now: Date.init, sleep: { _ in })
        let store = AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        frontend = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "",
                           orgId: "org-f", accountUuid: "uuid-f", plan: .max5x, status: .active, source: .manual)
        backend = Account(id: UUID(), name: "be", email: "backend@gotitapp.co", chromeProfilePath: "",
                          orgId: "org-b", accountUuid: "uuid-b", plan: .max5x, status: .active, source: .manual)
        store.addAccount(frontend)
        store.addAccount(backend)
        let soon = Int((Date().timeIntervalSince1970 + 8 * 3600) * 1000)
        let later = Int((Date().timeIntervalSince1970 + 20 * 86400) * 1000)
        try JSONSerialization.data(withJSONObject: ["oauthAccount": ["emailAddress": "frontend@gotitapp.co"]])
            .write(to: configURL)
        try slot.overwrite(OAuthCredential(object: ["refreshToken": "f1", "expiresAt": soon,
                                                    "refreshTokenExpiresAt": later])!)
        try ccVault.save(VaultEntry(
            oauth: OAuthCredential(object: ["refreshToken": "b1", "expiresAt": soon,
                                            "refreshTokenExpiresAt": later + 1000])!,
            oauthAccount: OAuthAccountJSON.canonical(["emailAddress": "backend@gotitapp.co"])!), for: backend.id)

        desktopConfig.grant = ("uuid-f", OAuthCredential(object: ["refreshToken": "df", "expiresAt": soon])!)
        desktopVault.items[backend.id] = VaultEntry(
            oauth: OAuthCredential(object: ["refreshToken": "db", "expiresAt": soon])!,
            oauthAccount: OAuthAccountJSON.canonical(["emailAddress": "backend@gotitapp.co"])!)
        let desktop = ClaudeDesktopSwitcher(config: desktopConfig, vault: desktopVault, process: process,
                                            now: Date.init)

        vm = DashboardViewModel(accountStore: store, ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                                ccSwitcher: ccSwitcher, desktopSwitcher: desktop)
        await vm.refreshAll()
        // Frontend still has quota left, so a Switch asks first.
        let index = try XCTUnwrap(vm.accountStates.firstIndex { $0.id == frontend.id })
        vm.accountStates[index].usage = UsageData(fiveHour: UsageLimit(utilization: 40, resetsAt: nil),
                                                  sevenDay: UsageLimit(utilization: 60, resetsAt: nil))
    }

    override func tearDown() async throws {
        AppDefaults.shared.removeObject(forKey: AppDefaults.switchDesktopEnabledKey)
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(at: configURL)
    }

    func testCheckboxPreTickedWhenDesktopRunningAndEnabled() async throws {
        vm.switchDesktopEnabled = true
        process.isRunning = true
        await vm.requestSwitchClaudeCode(to: backend.id)
        let pending = try XCTUnwrap(vm.pendingSwitch)
        XCTAssertTrue(pending.desktopRunning)
        XCTAssertTrue(pending.switchDesktop)
    }

    func testNoCheckboxWhenDesktopNotRunning() async throws {
        vm.switchDesktopEnabled = true
        process.isRunning = false
        await vm.requestSwitchClaudeCode(to: backend.id)
        let pending = try XCTUnwrap(vm.pendingSwitch)
        XCTAssertFalse(pending.desktopRunning)
        XCTAssertFalse(pending.switchDesktop)
    }

    func testDeclineCloseLeavesDesktopUntouched() async throws {
        vm.switchDesktopEnabled = true
        process.isRunning = true
        await vm.requestSwitchClaudeCode(to: backend.id)
        await vm.confirmSwitch(try XCTUnwrap(vm.pendingSwitch))

        // Claude Code moved; Desktop waits for the close confirmation.
        XCTAssertEqual(vm.activeClaudeCodeEmail, "backend@gotitapp.co")
        XCTAssertEqual(vm.pendingDesktopClose?.targetId, backend.id)
        XCTAssertEqual(process.quitCalls, 0)

        vm.cancelDesktopClose()
        XCTAssertNil(vm.pendingDesktopClose)
        XCTAssertEqual(process.quitCalls, 0)
        XCTAssertTrue(desktopConfig.writes.isEmpty)
        XCTAssertEqual(desktopConfig.grant?.accountUuid, "uuid-f")
    }

    func testAgreeingToCloseQuitsThenSwitchesDesktop() async throws {
        vm.switchDesktopEnabled = true
        process.isRunning = true
        await vm.requestSwitchClaudeCode(to: backend.id)
        await vm.confirmSwitch(try XCTUnwrap(vm.pendingSwitch))
        await vm.confirmDesktopClose(try XCTUnwrap(vm.pendingDesktopClose))

        XCTAssertEqual(process.quitCalls, 1)
        XCTAssertEqual(desktopConfig.writes, ["uuid-b"])
        XCTAssertNil(vm.pendingDesktopClose)
        XCTAssertFalse(vm.desktopDiffersFromClaudeCode())
        XCTAssertFalse(vm.desktopDiffers)
    }

    func testLookAheadWritesWhenDesktopClosed() async throws {
        vm.switchDesktopEnabled = true
        process.isRunning = false
        await vm.requestSwitchClaudeCode(to: backend.id)
        await vm.confirmSwitch(try XCTUnwrap(vm.pendingSwitch))

        XCTAssertEqual(desktopConfig.writes, ["uuid-b"])
        XCTAssertEqual(process.quitCalls, 0)
        XCTAssertNil(vm.pendingDesktopClose)
    }

    func testDisabledSettingNeverTouchesDesktop() async throws {
        vm.switchDesktopEnabled = false
        process.isRunning = true
        await vm.requestSwitchClaudeCode(to: backend.id)
        await vm.confirmSwitch(try XCTUnwrap(vm.pendingSwitch))
        XCTAssertNil(vm.pendingDesktopClose)
        XCTAssertEqual(process.quitCalls, 0)
        XCTAssertTrue(desktopConfig.writes.isEmpty)
    }

    func testDetailButtonVisibleWhenAccountsDiffer() async throws {
        XCTAssertFalse(vm.desktopDiffersFromClaudeCode())
        XCTAssertFalse(vm.desktopDiffers)
        await vm.switchClaudeCode(to: backend.id)
        XCTAssertTrue(vm.desktopDiffersFromClaudeCode())
        // The stored flag the pane reads follows the CLI switch, then the Desktop switch.
        XCTAssertTrue(vm.desktopDiffers)
        await vm.switchDesktop(to: backend.id)
        XCTAssertFalse(vm.desktopDiffers)
    }
}
