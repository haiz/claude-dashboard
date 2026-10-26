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

/// Auto Switch's Claude Desktop look-ahead: with Desktop closed, the setting on and the target
/// captured for Desktop, `performAutoSwitch` also writes Desktop; otherwise only the CLI moves.
@MainActor
final class AutoSwitchDesktopTests: XCTestCase {
    private var suite: String!
    private var configURL: URL!
    private let desktopConfig = FakeConfig()
    private let desktopVault = FakeVault()
    private let process = FakeProcess()
    private var vm: DashboardViewModel!
    private var backend: Account!

    override func setUp() async throws {
        suite = StoreFixture.makeSuiteName()
        configURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("AutoSwitchDesktopTests-\(UUID().uuidString).json")
        AppDefaults.shared.removeObject(forKey: AppDefaults.autoSwitchDesktopEnabledKey)

        let kc = InMemoryKeychain()
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let ccVault = KeychainCredentialVault(keychain: kc)
        let ccSwitcher = ClaudeCodeSwitcher(slot: slot, vault: ccVault, config: ClaudeConfigFile(fileURL: configURL),
                                            now: Date.init, sleep: { _ in })
        let store = AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        let frontend = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "",
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
        let desktop = ClaudeDesktopSwitcher(config: desktopConfig, vault: desktopVault, process: process,
                                            now: Date.init)
        vm = DashboardViewModel(accountStore: store, ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                                ccSwitcher: ccSwitcher, desktopSwitcher: desktop)
        await vm.refreshAll()
        // Only after the refresh capture, so a test controls whether the target is captured.
        desktopVault.items[backend.id] = VaultEntry(
            oauth: OAuthCredential(object: ["refreshToken": "db", "expiresAt": soon])!,
            oauthAccount: OAuthAccountJSON.canonical(["emailAddress": "backend@gotitapp.co"])!)
        desktopConfig.writes = []
        vm.autoSwitchDesktopEnabled = true
    }

    override func tearDown() async throws {
        AppDefaults.shared.removeObject(forKey: AppDefaults.autoSwitchDesktopEnabledKey)
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(at: configURL)
    }

    func testLookAheadWritesWhenDesktopClosed() async throws {
        process.isRunning = false
        try await vm.performAutoSwitch(to: backend.id)
        XCTAssertEqual(vm.activeClaudeCodeEmail, "backend@gotitapp.co")
        XCTAssertEqual(desktopConfig.writes, ["uuid-b"])
        XCTAssertEqual(process.quitCalls, 0)
    }

    func testLookAheadSkippedWhenDesktopRunning() async throws {
        process.isRunning = true
        try await vm.performAutoSwitch(to: backend.id)
        XCTAssertEqual(vm.activeClaudeCodeEmail, "backend@gotitapp.co")
        XCTAssertTrue(desktopConfig.writes.isEmpty)
        XCTAssertEqual(process.quitCalls, 0)
    }

    func testLookAheadSkipsUncaptured() async throws {
        process.isRunning = false
        desktopVault.items[backend.id] = nil
        try await vm.performAutoSwitch(to: backend.id)
        XCTAssertEqual(vm.activeClaudeCodeEmail, "backend@gotitapp.co")
        XCTAssertTrue(desktopConfig.writes.isEmpty)
        XCTAssertNil(vm.switchMessage)
    }

    func testLookAheadOffWhenSettingDisabled() async throws {
        process.isRunning = false
        vm.autoSwitchDesktopEnabled = false
        try await vm.performAutoSwitch(to: backend.id)
        XCTAssertTrue(desktopConfig.writes.isEmpty)
    }
}
