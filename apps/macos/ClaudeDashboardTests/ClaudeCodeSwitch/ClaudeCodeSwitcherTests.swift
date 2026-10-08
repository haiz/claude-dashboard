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

    /// `refreshTokenExpiresAt` is fixed per grant (spec fact 4), so by default it is derived
    /// from the token's first letter: "f1" and its rotation "f2" share one deadline, "b1"
    /// has another, and the identity guard can tell the accounts apart.
    private func cred(_ refresh: String, expiresIn: TimeInterval = 8 * 3600,
                      refreshExpiresIn: TimeInterval? = nil) -> OAuthCredential {
        let ms = { (t: TimeInterval) in Int((self.now.timeIntervalSince1970 + t) * 1000) }
        let grant = refreshExpiresIn ?? 20 * 86400 + TimeInterval(refresh.unicodeScalars.first?.value ?? 0)
        return OAuthCredential(object: ["accessToken": "a-\(refresh)", "refreshToken": refresh,
                                        "expiresAt": ms(expiresIn), "refreshTokenExpiresAt": ms(grant)])!
    }

    private func accountJSON(_ email: String) -> Data {
        OAuthAccountJSON.canonical(["emailAddress": email, "organizationName": "MathGPT.ai"])!
    }

    /// Claude Code is signed in as `email` with `credential`.
    private func signIn(_ email: String, _ credential: OAuthCredential) throws {
        try JSONSerialization.data(withJSONObject: ["oauthAccount": OAuthAccountJSON.object(accountJSON(email))!, "projects": [:]])
            .write(to: configURL)
        try slot.overwrite(credential)
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
        try slot.overwrite(OAuthCredential(object: ["accessToken": "", "refreshToken": "", "expiresAt": 0])!)

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
        try slot.overwrite(cred("f2"))

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
            if polls == 2 { try? self.slot.overwrite(self.cred("f2")) }   // a session refreshed
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
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
    }

    func testSwitchRefusesLiveCredentialWhenConfigNamesNoAccount() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try Data("{}".utf8).write(to: configURL)
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        do { try await makeSwitcher().switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .activeAccountUnknown) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
    }

    func testSwitchProceedsWhenSlotIsEmptyAndConfigNamesNoAccount() async throws {
        try Data("{}".utf8).write(to: configURL)
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try await makeSwitcher().switchTo(backend, accounts: [backend])
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
    }

    func testSwitchReportsRollbackFailedWhenCredentialCannotBeRestored() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        kc.failWritesAfter = 2   // vault save of frontend, slot write of backend; the rollback write fails
        let switcher = makeSwitcher(config: FailingConfig(inner: ClaudeConfigFile(fileURL: configURL)))

        do { try await switcher.switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .rollbackFailed) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
    }

    // MARK: identity guard

    /// Writes the target's `oauthAccount` and then sees it undone, as when a running Claude
    /// Code process rewrites `~/.claude.json` from its own memory: the switch ends in
    /// `verifyFailed` with the entry holding the target and the config naming the previous account.
    private struct UndoneConfig: ClaudeConfigAccountFile {
        let inner: ClaudeConfigFile
        func readOAuthAccount() throws -> Data? { try inner.readOAuthAccount() }
        func writeOAuthAccount(_ json: Data) throws {
            let before = try inner.readOAuthAccount()
            try inner.writeOAuthAccount(json)
            if let before { try inner.writeOAuthAccount(before) }
        }
    }

    private struct FailingConfig: ClaudeConfigAccountFile {
        let inner: ClaudeConfigFile
        func readOAuthAccount() throws -> Data? { try inner.readOAuthAccount() }
        func writeOAuthAccount(_ json: Data) throws { throw ClaudeConfigFileError.unreadable }
    }

    func testCaptureAfterVerifyFailedKeepsActiveAccountsVaultCopy() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        let undone = makeSwitcher(config: UndoneConfig(inner: ClaudeConfigFile(fileURL: configURL)))
        do { try await undone.switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .verifyFailed) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")

        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend, backend]), .mismatch(frontend.id))

        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
        XCTAssertEqual(try vault.load(backend.id)?.oauth.refreshToken, "b1")
    }

    /// After `verifyFailed` the entry holds backend's credential, already in backend's vault:
    /// switching away overwrites nothing unsaved, so it proceeds without filing it under frontend.
    func testSwitchAfterVerifyFailedProceedsWhenEntryIsItsOwnersSavedCopy() async throws {
        let other = Self.account("other@gotitapp.co")
        let accounts = [frontend, backend, other]
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try vault.save(VaultEntry(oauth: cred("o1"), oauthAccount: accountJSON("other@gotitapp.co")), for: other.id)
        let undone = makeSwitcher(config: UndoneConfig(inner: ClaudeConfigFile(fileURL: configURL)))
        do { try await undone.switchTo(backend, accounts: accounts); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .verifyFailed) }

        try await makeSwitcher().switchTo(other, accounts: accounts)

        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "o1")
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
        XCTAssertEqual(try vault.load(backend.id)?.oauth.refreshToken, "b1")
    }

    /// The entry holds a rotated backend token the vault never saw while the config names
    /// frontend: it is nobody's saved copy, so overwriting it would lose backend's login.
    func testSwitchRefusesWhenEntryHoldsAnotherAccountsUnsavedToken() async throws {
        let other = Self.account("other@gotitapp.co")
        let accounts = [frontend, backend, other]
        try signIn("frontend@gotitapp.co", cred("b2"))
        try vault.save(VaultEntry(oauth: cred("f1"), oauthAccount: accountJSON("frontend@gotitapp.co")), for: frontend.id)
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try vault.save(VaultEntry(oauth: cred("o1"), oauthAccount: accountJSON("other@gotitapp.co")), for: other.id)

        do { try await makeSwitcher().switchTo(other, accounts: accounts); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .activeAccountUnknown) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b2")
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
    }

    func testCaptureAfterRollbackFailedKeepsActiveAccountsVaultCopy() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        kc.failWritesAfter = 2   // vault save of frontend, slot write of backend; the rollback write fails
        let failing = makeSwitcher(config: FailingConfig(inner: ClaudeConfigFile(fileURL: configURL)))
        do { try await failing.switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .rollbackFailed) }
        kc.failWritesAfter = nil

        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend, backend]), .mismatch(frontend.id))

        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
    }

    func testSwitchRefusesTargetWhoseVaultCopyNamesAnotherEmail() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("o1"), oauthAccount: accountJSON("other@gotitapp.co")), for: backend.id)
        do { try await makeSwitcher().switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .notCaptured) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(ClaudeConfigFile(fileURL: configURL).readOAuthAccount())), "frontend@gotitapp.co")
    }

    // MARK: compare-and-swap

    /// Wraps the real slot. `beforeWrite(n)` runs before the n-th `writeOAuth` (1-based)
    /// reaches the Keychain, e.g. to land a Claude Code refresh between the switch's read
    /// and its write; `afterWrite(n)` runs after it, e.g. to fail a write that did land.
    private final class HookedSlot: ClaudeCodeCredentialSlot {
        let inner: ClaudeCodeCredentialSlot
        var beforeWrite: ((Int) throws -> Void)?
        var afterWrite: ((Int) throws -> Void)?
        private(set) var writes = 0
        init(_ inner: ClaudeCodeCredentialSlot) { self.inner = inner }
        func readOAuth() throws -> OAuthCredential? { try inner.readOAuth() }
        func writeOAuth(_ credential: OAuthCredential, expecting: OAuthCredential?) throws {
            writes += 1
            try beforeWrite?(writes)
            try inner.writeOAuth(credential, expecting: expecting)
            try afterWrite?(writes)
        }
    }

    private func makeSwitcher(slot hooked: HookedSlot) -> ClaudeCodeSwitcher {
        ClaudeCodeSwitcher(slot: hooked, vault: vault, config: ClaudeConfigFile(fileURL: configURL),
                           now: { [unowned self] in self.now }, sleep: { _ in })
    }

    func testSwitchRetriesWhenRefreshLandsBetweenReadAndWrite() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        let hooked = HookedSlot(slot)
        hooked.beforeWrite = { [unowned self] n in if n == 1 { try self.slot.overwrite(self.cred("f2")) } }

        try await makeSwitcher(slot: hooked).switchTo(backend, accounts: [frontend, backend])

        XCTAssertEqual(hooked.writes, 2)
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f2")
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(ClaudeConfigFile(fileURL: configURL).readOAuthAccount())), "backend@gotitapp.co")
    }

    func testSwitchGivesUpWithKeychainErrorWhenEntryKeepsChanging() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        let hooked = HookedSlot(slot)
        hooked.beforeWrite = { [unowned self] n in try self.slot.overwrite(self.cred("f\(n + 1)")) }

        do { try await makeSwitcher(slot: hooked).switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .keychain) }

        XCTAssertEqual(hooked.writes, ClaudeCodeSwitcher.maxWriteAttempts)
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f4")
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(ClaudeConfigFile(fileURL: configURL).readOAuthAccount())), "frontend@gotitapp.co")
    }

    /// Rollback after a config failure writes over the credential the switch wrote only:
    /// a session that refreshed the target in between keeps its newer token.
    func testConfigRollbackNeverOverwritesANewerTargetToken() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        let hooked = HookedSlot(slot)
        hooked.beforeWrite = { [unowned self] n in if n == 2 { try self.slot.overwrite(self.cred("b2")) } }
        let switcher = ClaudeCodeSwitcher(slot: hooked, vault: vault,
                                          config: FailingConfig(inner: ClaudeConfigFile(fileURL: configURL)),
                                          now: { [unowned self] in self.now }, sleep: { _ in })

        do { try await switcher.switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .rollbackFailed) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b2")
    }

    /// The slot write lands but reports failure (e.g. `writeNotPersisted`): the switch
    /// restores the previous credential and only then reports `.keychain` ("nothing changed").
    func testSwitchRestoresEntryWhenSlotWriteFailsAfterLanding() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        let hooked = HookedSlot(slot)
        hooked.afterWrite = { n in if n == 1 { throw KeychainError.writeNotPersisted } }

        do { try await makeSwitcher(slot: hooked).switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .keychain) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(ClaudeConfigFile(fileURL: configURL).readOAuthAccount())), "frontend@gotitapp.co")
    }

    func testSwitchReportsRollbackFailedWhenLandedSlotWriteCannotBeUndone() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        let hooked = HookedSlot(slot)
        hooked.afterWrite = { _ in throw KeychainError.writeNotPersisted }
        hooked.beforeWrite = { n in if n == 2 { throw KeychainError.commandFailed(status: 1) } }

        do { try await makeSwitcher(slot: hooked).switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .rollbackFailed) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
    }

    // MARK: refresh window boundaries

    private func sleepCount(expiresIn: TimeInterval) async throws -> Int {
        try signIn("frontend@gotitapp.co", cred("f1", expiresIn: expiresIn))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])
        return sleeps.count
    }

    func testWaitsExactly60SecondsPastExpiry() async throws { let n = try await sleepCount(expiresIn: -60); XCTAssertEqual(n, 10) }
    func testWaitsExactly300SecondsBeforeExpiry() async throws { let n = try await sleepCount(expiresIn: 300); XCTAssertEqual(n, 10) }
    func testDoesNotWait301SecondsBeforeExpiry() async throws { let n = try await sleepCount(expiresIn: 301); XCTAssertEqual(n, 0) }
    func testDoesNotWait61SecondsPastExpiry() async throws { let n = try await sleepCount(expiresIn: -61); XCTAssertEqual(n, 0) }

    // MARK: concurrency

    func testConcurrentCaptureAndSwitchNeverSavesTargetCredentialIntoActiveAccount() async throws {
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        for _ in 0..<50 {
            try signIn("frontend@gotitapp.co", cred("f1"))
            let switcher = makeSwitcher()
            let accounts = [frontend, backend]
            let (f, b) = (frontend, backend)
            await withTaskGroup(of: Void.self) { group in
                group.addTask { _ = try? switcher.capture(accounts: accounts) }
                group.addTask { try? await switcher.switchTo(b, accounts: accounts) }
                group.addTask { _ = try? switcher.capture(accounts: accounts) }
            }
            XCTAssertNotEqual(try vault.load(f.id)?.oauth.refreshToken, "b1")
            XCTAssertNotEqual(try vault.load(b.id)?.oauth.refreshToken, "f1")
        }
    }

    // MARK: forget

    func testForgetDeletesTheVaultCopyAndSurvivesFailure() throws {
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try vault.save(VaultEntry(oauth: cred("f1"), oauthAccount: accountJSON("frontend@gotitapp.co")), for: frontend.id)
        makeSwitcher().forget(accountId: backend.id)
        XCTAssertNil(try vault.load(backend.id))
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")

        struct FailingVault: CredentialVaulting {
            func load(_ accountId: UUID) throws -> VaultEntry? { nil }
            func save(_ entry: VaultEntry, for accountId: UUID) throws {}
            func delete(_ accountId: UUID) throws { throw KeychainError.commandFailed(status: 1) }
        }
        ClaudeCodeSwitcher(slot: slot, vault: FailingVault(), config: ClaudeConfigFile(fileURL: configURL),
                           now: Date.init, sleep: { _ in }).forget(accountId: backend.id)   // logs, no throw
    }

    func testLiveIsNilUnderTests() {
        XCTAssertNil(ClaudeCodeSwitcher.live(isRunningTests: true))
    }
}
