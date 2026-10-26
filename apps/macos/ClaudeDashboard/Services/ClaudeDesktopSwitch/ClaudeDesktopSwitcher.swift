import Foundation

/// What the Desktop Switch button can do for one account.
enum DesktopSwitchAvailability: Equatable {
    /// Claude Desktop is already signed in as this account.
    case active
    case ready
    /// No Desktop grant of its own yet: the Switch captures one through the browser.
    case notCaptured
    /// The vault copy is past `refreshTokenExpiresAt`: the Switch captures a new one.
    case loginExpired
}

enum DesktopSwitchError: Error, Equatable {
    /// Claude Desktop is running; the caller quits it first (with the user's consent).
    case desktopRunning
    /// No usable vault grant and no way to capture one (no provisioner, or no browser profile).
    case notCaptured
    case loginExpired
    /// Desktop holds a live login the dashboard cannot save (unknown account, or the save
    /// failed); overwriting it would discard its refresh token.
    case unknownOutgoingAccount(email: String?)
    /// `config.json` could not be written, or the target lacks an account or org uuid.
    case config
    /// The written grant did not read back as the target's.
    case verifyFailed

    /// Case name only, safe to log (no emails).
    var logName: String {
        switch self {
        case .desktopRunning: return "desktopRunning"
        case .notCaptured: return "notCaptured"
        case .loginExpired: return "loginExpired"
        case .unknownOutgoingAccount: return "unknownOutgoingAccount"
        case .config: return "config"
        case .verifyFailed: return "verifyFailed"
        }
    }
}

/// Swaps Claude Desktop's signed-in account using one independent grant per account, kept
/// in its own vault so Desktop and Claude Code never share a refresh-token chain.
///
/// `captureActive` and the synchronous part of `switchTo` share one lock. `switchTo` never
/// writes while Desktop runs (Electron rewrites `config.json` on quit) and never overwrites
/// a live outgoing grant it could not first save.
///
///     let switcher = ClaudeDesktopSwitcher.live(isRunningTests: AppDefaults.isRunningTests())
///     if switcher?.isDesktopRunning == true { /* ask, quit off the main actor */ }
///     try await switcher?.switchTo(account, accounts: store.accounts, provision: provisionClosure)
final class ClaudeDesktopSwitcher: @unchecked Sendable {
    private let config: ClaudeDesktopConfigReadingWriting
    private let vault: CredentialVaulting
    private let process: ClaudeDesktopProcessControlling
    private let now: () -> Date
    /// Never held across an `await`.
    private let lock = NSLock()

    init(config: ClaudeDesktopConfigReadingWriting, vault: CredentialVaulting,
         process: ClaudeDesktopProcessControlling, now: @escaping () -> Date) {
        self.config = config
        self.vault = vault
        self.process = process
        self.now = now
    }

    /// The real switcher, or nil under XCTest (the test host is the real app), or when the
    /// Safe Storage password is unavailable.
    static func live(isRunningTests: Bool) -> ClaudeDesktopSwitcher? {
        guard !isRunningTests, let password = ClaudeDesktopSafeStorage.password() else { return nil }
        return ClaudeDesktopSwitcher(
            config: ClaudeDesktopConfigFile(fileURL: ClaudeDesktopConfigFile.defaultURL, password: password),
            vault: KeychainDesktopVault(keychain: SecurityCLIKeychain()),
            process: ClaudeDesktopProcess(),
            now: Date.init)
    }

    var isDesktopRunning: Bool { process.isRunning }

    /// Desktop's `lastKnownAccountUuid`, when it holds a readable grant.
    func activeAccountUuid() -> String? {
        (try? config.readActiveGrant())?.accountUuid
    }

    // MARK: Availability

    func availability(for accounts: [Account]) -> [UUID: DesktopSwitchAvailability] {
        let activeUuid = activeAccountUuid()
        var map: [UUID: DesktopSwitchAvailability] = [:]
        for account in accounts {
            if let activeUuid, account.accountUuid == activeUuid {
                map[account.id] = .active
                continue
            }
            guard let entry = (try? vault.load(account.id)) ?? nil else {
                map[account.id] = .notCaptured
                continue
            }
            map[account.id] = isExpired(entry) ? .loginExpired : .ready
        }
        return map
    }

    // MARK: Capture

    /// Opportunistic: saves Desktop's live grant to the vault of the dashboard account it names.
    func captureActive(accounts: [Account]) throws {
        lock.lock()
        defer { lock.unlock() }
        guard let grant = try config.readActiveGrant(), !grant.oauth.isBlank,
              let account = Self.account(uuid: grant.accountUuid, in: accounts) else { return }
        try saveOutgoing(grant.oauth, for: account)
    }

    // MARK: Switch

    func switchTo(_ target: Account, accounts: [Account],
                  provision: ((ProvisionInput) async throws -> VaultEntry)?) async throws {
        do {
            // Browser capture can take minutes: outside the lock, before anything is touched.
            try guardNotRunning()
            let entry = try await resolveGrant(for: target, provision: provision)
            try performWrite(entry, target: target, accounts: accounts)
        } catch let error as DesktopSwitchError {
            print("[ClaudeDesktopSwitcher] switch -> \(target.id): \(error.logName)")
            throw error
        }
    }

    private func guardNotRunning() throws {
        if process.isRunning { throw DesktopSwitchError.desktopRunning }
    }

    /// The target's vault grant, or a fresh browser capture stored in the vault.
    private func resolveGrant(for target: Account,
                              provision: ((ProvisionInput) async throws -> VaultEntry)?) async throws -> VaultEntry {
        var expired = false
        if let entry = (try? vault.load(target.id)) ?? nil, !entry.oauth.isBlank, Self.names(entry, target) {
            if !isExpired(entry) { return entry }
            expired = true
        }
        guard let provision, let email = target.email, !target.chromeProfilePath.isEmpty else {
            throw expired ? DesktopSwitchError.loginExpired : DesktopSwitchError.notCaptured
        }
        let input = ProvisionInput(email: email, browser: target.browser, profilePath: target.chromeProfilePath)
        let entry = try await provision(input)
        guard !entry.oauth.isBlank, !isExpired(entry), Self.names(entry, target) else {
            throw DesktopSwitchError.notCaptured
        }
        try store(entry, for: target.id)
        return entry
    }

    /// Synchronous and locked: no capture or second switch interleaves between reading the
    /// outgoing grant and writing the target's.
    private func performWrite(_ entry: VaultEntry, target: Account, accounts: [Account]) throws {
        lock.lock()
        defer { lock.unlock() }

        // Re-check under the lock: Desktop may have launched during the browser capture.
        try guardNotRunning()
        guard let accountUuid = target.accountUuid, let orgUuid = target.orgId else {
            throw DesktopSwitchError.config
        }

        let outgoing: (accountUuid: String, oauth: OAuthCredential)?
        do { outgoing = try config.readActiveGrant() } catch {
            throw DesktopSwitchError.unknownOutgoingAccount(email: nil)
        }
        if let outgoing, !outgoing.oauth.isBlank {
            // Already signed in as the target with a live grant: nothing to do.
            if outgoing.accountUuid == accountUuid { return }
            guard let account = Self.account(uuid: outgoing.accountUuid, in: accounts) else {
                throw DesktopSwitchError.unknownOutgoingAccount(email: nil)
            }
            do { try saveOutgoing(outgoing.oauth, for: account) } catch {
                throw DesktopSwitchError.unknownOutgoingAccount(email: account.email)
            }
        }

        do { try config.writeGrant(entry.oauth, accountUuid: accountUuid, orgUuid: orgUuid) } catch {
            throw DesktopSwitchError.config
        }
        guard let written = try? config.readActiveGrant(), written.accountUuid == accountUuid,
              written.oauth.refreshToken == entry.oauth.refreshToken else {
            throw DesktopSwitchError.verifyFailed
        }
    }

    // MARK: Store / Forget

    /// Saves a grant the provisioner minted for an account.
    func store(_ entry: VaultEntry, for accountId: UUID) throws {
        lock.lock()
        defer { lock.unlock() }
        try vault.save(entry, for: accountId)
    }

    /// Drops a removed account's vault copy. Never throws: a failure leaves an orphan item.
    func forget(accountId: UUID) {
        lock.lock()
        defer { lock.unlock() }
        do { try vault.delete(accountId) }
        catch { print("[ClaudeDesktopSwitcher] forget \(accountId) failed: \(error)") }
    }

    // MARK: Helpers

    /// Caller holds the lock. Keeps the stored `oauthAccount` when there is one.
    private func saveOutgoing(_ oauth: OAuthCredential, for account: Account) throws {
        let existing = (try? vault.load(account.id)) ?? nil
        if existing?.oauth == oauth { return }
        var info: [String: Any] = ["uuid": account.accountUuid ?? ""]
        if let email = account.email { info["emailAddress"] = email }
        let oauthAccount = existing?.oauthAccount ?? OAuthAccountJSON.canonical(info) ?? Data()
        try vault.save(VaultEntry(oauth: oauth, oauthAccount: oauthAccount), for: account.id)
    }

    /// False only when the entry names an email other than the target's.
    private static func names(_ entry: VaultEntry, _ target: Account) -> Bool {
        guard let email = OAuthAccountJSON.email(entry.oauthAccount), let expected = target.email else { return true }
        return email.caseInsensitiveCompare(expected) == .orderedSame
    }

    private func isExpired(_ entry: VaultEntry) -> Bool {
        guard let deadline = entry.oauth.refreshTokenExpiresAt else { return false }
        return deadline <= now()
    }

    private static func account(uuid: String, in accounts: [Account]) -> Account? {
        accounts.first { $0.accountUuid == uuid }
    }
}
