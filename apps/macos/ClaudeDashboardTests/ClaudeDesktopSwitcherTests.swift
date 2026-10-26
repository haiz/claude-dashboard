@testable import ClaudeDashboard
import XCTest

private final class FakeConfig: ClaudeDesktopConfigReadingWriting {
    var grant: (accountUuid: String, oauth: OAuthCredential)?
    var writes: [(oauth: OAuthCredential, accountUuid: String, orgUuid: String)] = []
    var failWrite = false

    func readActiveGrant() throws -> (accountUuid: String, oauth: OAuthCredential)? { grant }
    func writeGrant(_ oauth: OAuthCredential, accountUuid: String, orgUuid: String) throws {
        if failWrite { throw ClaudeDesktopConfigError.unreadableConfig }
        writes.append((oauth, accountUuid, orgUuid))
        grant = (accountUuid, oauth)
    }
}

private final class FakeVault: CredentialVaulting {
    var items: [UUID: VaultEntry] = [:]
    var failSave = false
    func load(_ accountId: UUID) throws -> VaultEntry? { items[accountId] }
    func save(_ entry: VaultEntry, for accountId: UUID) throws {
        if failSave { throw ClaudeDesktopConfigError.encryptionFailed }
        items[accountId] = entry
    }
    func delete(_ accountId: UUID) throws { items[accountId] = nil }
}

private final class FakeProcess: ClaudeDesktopProcessControlling {
    var isRunning = false
    var quitCalls = 0
    func quit() -> Bool { quitCalls += 1; return true }
}

final class ClaudeDesktopSwitcherTests: XCTestCase {
    private let config = FakeConfig()
    private let vault = FakeVault()
    private let process = FakeProcess()
    private let now = Date(timeIntervalSince1970: 1_800_000_000)
    private var switcher: ClaudeDesktopSwitcher!

    private let a = Account(id: UUID(), name: "a", email: "a@x.co", chromeProfilePath: "/p/a", chromeProfileName: nil,
                             orgId: "org-a", accountUuid: "uuid-a", plan: .max5x, status: .active, source: .browser)
    private let b = Account(id: UUID(), name: "b", email: "b@x.co", chromeProfilePath: "/p/b", chromeProfileName: nil,
                             orgId: "org-b", accountUuid: "uuid-b", plan: .max5x, status: .active, source: .browser)
    private var accounts: [Account] { [a, b] }

    override func setUp() {
        switcher = ClaudeDesktopSwitcher(config: config, vault: vault, process: process, now: { self.now })
    }

    private func oauth(_ refresh: String, refreshExpiresMs: Int? = nil) -> OAuthCredential {
        var object: [String: Any] = ["accessToken": "acc-\(refresh)", "refreshToken": refresh,
                                     "expiresAt": 1_900_000_000_000]
        if let ms = refreshExpiresMs { object["refreshTokenExpiresAt"] = ms }
        return OAuthCredential(object: object)!
    }

    private func entry(_ refresh: String, email: String, refreshExpiresMs: Int? = nil) -> VaultEntry {
        VaultEntry(oauth: oauth(refresh, refreshExpiresMs: refreshExpiresMs),
                   oauthAccount: OAuthAccountJSON.canonical(["emailAddress": email])!)
    }

    private func assertThrows(_ expected: DesktopSwitchError, file: StaticString = #filePath, line: UInt = #line,
                              _ body: () async throws -> Void) async {
        do { try await body(); XCTFail("expected \(expected)", file: file, line: line) }
        catch { XCTAssertEqual(error as? DesktopSwitchError, expected, file: file, line: line) }
    }

    func testSwitchRefusesWhileDesktopRunning() async {
        process.isRunning = true
        vault.items[a.id] = entry("ra", email: "a@x.co")
        config.grant = ("uuid-b", oauth("rb"))
        await assertThrows(.desktopRunning) { try await switcher.switchTo(a, accounts: accounts, provision: nil) }
        XCTAssertTrue(config.writes.isEmpty)
        XCTAssertNil(vault.items[b.id])
        XCTAssertEqual(process.quitCalls, 0)
    }

    func testSwitchCapturesOutgoingBeforeOverwrite() async throws {
        vault.items[a.id] = entry("ra", email: "a@x.co")
        vault.items[b.id] = entry("rb-old", email: "b@x.co")
        config.grant = ("uuid-b", oauth("rb-fresh"))
        try await switcher.switchTo(a, accounts: accounts, provision: nil)
        XCTAssertEqual(vault.items[b.id]?.oauth.refreshToken, "rb-fresh")
        XCTAssertEqual(OAuthAccountJSON.email(vault.items[b.id]!.oauthAccount), "b@x.co")
        XCTAssertEqual(config.writes.count, 1)
        XCTAssertEqual(config.grant?.accountUuid, "uuid-a")
    }

    func testSwitchCapturesOutgoingWithNoPriorVaultEntry() async throws {
        vault.items[a.id] = entry("ra", email: "a@x.co")
        config.grant = ("uuid-b", oauth("rb-fresh"))
        try await switcher.switchTo(a, accounts: accounts, provision: nil)
        XCTAssertEqual(vault.items[b.id]?.oauth.refreshToken, "rb-fresh")
        XCTAssertEqual(OAuthAccountJSON.email(vault.items[b.id]!.oauthAccount), "b@x.co")
    }

    func testSwitchRefusesWhenOutgoingCannotBeSaved() async {
        vault.items[a.id] = entry("ra", email: "a@x.co")
        config.grant = ("uuid-b", oauth("rb"))
        vault.failSave = true
        await assertThrows(.unknownOutgoingAccount(email: "b@x.co")) {
            try await switcher.switchTo(a, accounts: accounts, provision: nil)
        }
        XCTAssertTrue(config.writes.isEmpty)
        XCTAssertEqual(config.grant?.oauth.refreshToken, "rb")
    }

    func testSwitchGuardsUnknownOutgoingAccount() async {
        vault.items[a.id] = entry("ra", email: "a@x.co")
        config.grant = ("uuid-stranger", oauth("rs"))
        await assertThrows(.unknownOutgoingAccount(email: nil)) {
            try await switcher.switchTo(a, accounts: accounts, provision: nil)
        }
        XCTAssertTrue(config.writes.isEmpty)
        XCTAssertEqual(config.grant?.accountUuid, "uuid-stranger")
        XCTAssertEqual(config.grant?.oauth.refreshToken, "rs")
    }

    func testSwitchSkipsBlankOrAbsentOutgoing() async throws {
        vault.items[a.id] = entry("ra", email: "a@x.co")
        config.grant = nil
        try await switcher.switchTo(a, accounts: accounts, provision: nil)
        XCTAssertEqual(config.writes.count, 1)

        config.grant = ("uuid-stranger", OAuthCredential(object: ["accessToken": "", "refreshToken": ""])!)
        try await switcher.switchTo(a, accounts: accounts, provision: nil)
        XCTAssertEqual(config.writes.count, 2)
        XCTAssertNil(vault.items[b.id])
    }

    func testSwitchUsesVaultGrantWhenPresent() async throws {
        vault.items[a.id] = entry("ra", email: "a@x.co")
        var provisioned = false
        try await switcher.switchTo(a, accounts: accounts, provision: { _ in provisioned = true; throw CancellationError() })
        XCTAssertFalse(provisioned)
        XCTAssertEqual(config.writes.first?.oauth.refreshToken, "ra")
        XCTAssertEqual(config.writes.first?.accountUuid, "uuid-a")
        XCTAssertEqual(config.writes.first?.orgUuid, "org-a")
    }

    func testSwitchBrowserCapturesWhenMissing() async throws {
        var seen: ProvisionInput?
        try await switcher.switchTo(a, accounts: accounts, provision: { input in
            seen = input
            return self.entry("r-new", email: "a@x.co")
        })
        XCTAssertEqual(seen, ProvisionInput(email: "a@x.co", browser: .chrome, profilePath: "/p/a"))
        XCTAssertEqual(vault.items[a.id]?.oauth.refreshToken, "r-new")
        XCTAssertEqual(config.writes.first?.oauth.refreshToken, "r-new")
    }

    func testSwitchBrowserCapturesWhenVaultGrantExpired() async throws {
        let past = Int(now.timeIntervalSince1970 * 1000) - 1000
        vault.items[a.id] = entry("ra-dead", email: "a@x.co", refreshExpiresMs: past)
        try await switcher.switchTo(a, accounts: accounts, provision: { _ in self.entry("r-new", email: "a@x.co") })
        XCTAssertEqual(config.writes.first?.oauth.refreshToken, "r-new")
    }

    func testSwitchWithoutVaultGrantOrProvisionerIsNotCaptured() async {
        await assertThrows(.notCaptured) { try await switcher.switchTo(a, accounts: accounts, provision: nil) }
        let past = Int(now.timeIntervalSince1970 * 1000) - 1000
        vault.items[a.id] = entry("ra-dead", email: "a@x.co", refreshExpiresMs: past)
        await assertThrows(.loginExpired) { try await switcher.switchTo(a, accounts: accounts, provision: nil) }
        XCTAssertTrue(config.writes.isEmpty)
    }

    func testProvisionFailureLeavesConfigUntouched() async {
        config.grant = ("uuid-b", oauth("rb"))
        do {
            try await switcher.switchTo(a, accounts: accounts, provision: { _ in throw CancellationError() })
            XCTFail("expected throw")
        } catch {}
        XCTAssertTrue(config.writes.isEmpty)
        XCTAssertNil(vault.items[b.id])
    }

    func testDesktopStartingDuringBrowserCaptureIsRefused() async {
        await assertThrows(.desktopRunning) {
            try await switcher.switchTo(a, accounts: accounts, provision: { _ in
                self.process.isRunning = true
                return self.entry("r-new", email: "a@x.co")
            })
        }
        XCTAssertTrue(config.writes.isEmpty)
    }

    func testSwitchNeedsAccountAndOrgUuid() async {
        var bare = a
        bare.orgId = nil
        vault.items[a.id] = entry("ra", email: "a@x.co")
        await assertThrows(.config) { try await switcher.switchTo(bare, accounts: [bare, b], provision: nil) }
        XCTAssertTrue(config.writes.isEmpty)
    }

    func testWriteFailureIsConfigError() async {
        vault.items[a.id] = entry("ra", email: "a@x.co")
        config.failWrite = true
        await assertThrows(.config) { try await switcher.switchTo(a, accounts: accounts, provision: nil) }
    }

    func testAlreadyActiveLiveGrantIsNoOp() async throws {
        vault.items[a.id] = entry("ra", email: "a@x.co")
        config.grant = ("uuid-a", oauth("ra-live"))
        try await switcher.switchTo(a, accounts: accounts, provision: nil)
        XCTAssertTrue(config.writes.isEmpty)
    }

    func testAvailabilityNotCaptured() {
        let map = switcher.availability(for: accounts)
        XCTAssertEqual(map[a.id], .notCaptured)
        XCTAssertEqual(map[b.id], .notCaptured)
    }

    func testAvailabilityActiveReadyExpired() {
        config.grant = ("uuid-a", oauth("ra"))
        let past = Int(now.timeIntervalSince1970 * 1000) - 1000
        vault.items[b.id] = entry("rb", email: "b@x.co", refreshExpiresMs: past)
        XCTAssertEqual(switcher.availability(for: accounts), [a.id: .active, b.id: .loginExpired])
        vault.items[b.id] = entry("rb", email: "b@x.co")
        XCTAssertEqual(switcher.availability(for: accounts)[b.id], .ready)
        XCTAssertEqual(switcher.activeAccountUuid(), "uuid-a")
    }

    func testCaptureActiveSavesDashboardAccountOnly() throws {
        config.grant = ("uuid-b", oauth("rb"))
        try switcher.captureActive(accounts: accounts)
        XCTAssertEqual(vault.items[b.id]?.oauth.refreshToken, "rb")
        config.grant = ("uuid-stranger", oauth("rs"))
        try switcher.captureActive(accounts: accounts)
        XCTAssertEqual(vault.items.count, 1)
    }

    func testStoreForgetAndPassthroughs() throws {
        try switcher.store(entry("ra", email: "a@x.co"), for: a.id)
        XCTAssertNotNil(vault.items[a.id])
        switcher.forget(accountId: a.id)
        XCTAssertNil(vault.items[a.id])
        process.isRunning = true
        XCTAssertTrue(switcher.isDesktopRunning)
        XCTAssertNil(ClaudeDesktopSwitcher.live(isRunningTests: true))
    }
}
