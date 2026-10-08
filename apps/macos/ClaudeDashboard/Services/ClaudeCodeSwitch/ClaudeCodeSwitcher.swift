import Foundation

/// What the Switch button can do for one account.
enum SwitchAvailability: Equatable {
    /// Claude Code already uses this account.
    case active
    case ready
    /// The vault has no copy: run `/login` once with this account.
    case notCaptured
    /// The vault copy is past `refreshTokenExpiresAt`: run `/login` once.
    case loginExpired
    /// Active, but Claude Code blanked the credential after a failed refresh.
    case needsLogin
}

enum SwitchError: Error, Equatable {
    /// The vault has no copy of the target, or its copy names another email.
    case notCaptured
    case loginExpired
    /// Claude Code is signed in as an account the dashboard does not know; switching
    /// would discard its refresh token.
    case activeAccountNotInDashboard(email: String)
    /// Claude Code holds a live login but `~/.claude.json` names no account; switching
    /// would discard it.
    case activeAccountUnknown
    case keychain
    /// `~/.claude.json` could not be written; the previous credential was restored.
    case config
    /// The target's credential is in the Keychain but `~/.claude.json` still names the
    /// previous account, and the previous credential could not be restored: run `/login`.
    case rollbackFailed
    case verifyFailed

    /// Case name only, safe to log (no emails).
    var logName: String {
        switch self {
        case .notCaptured: return "notCaptured"
        case .loginExpired: return "loginExpired"
        case .activeAccountNotInDashboard: return "activeAccountNotInDashboard"
        case .activeAccountUnknown: return "activeAccountUnknown"
        case .keychain: return "keychain"
        case .config: return "config"
        case .rollbackFailed: return "rollbackFailed"
        case .verifyFailed: return "verifyFailed"
        }
    }
}

enum CaptureResult: Equatable {
    case noActiveAccount
    case blanked(UUID)
    case unchanged(UUID)
    case saved(UUID)
    /// The entry holds another dashboard account's credential while `~/.claude.json` names
    /// this account (after `verifyFailed` or `rollbackFailed`); nothing was saved.
    case mismatch(UUID)
}

/// Keeps every account's Claude Code login alive and swaps it into `~/.claude`.
///
/// `capture` and `switchTo` are serialized by one lock, so a refresh-time capture can never
/// read the config of one account and the credential of another.
///
/// `capture` runs on every dashboard refresh and copies the active credential into the
/// vault, because Claude Code rotates the refresh token on each refresh and an older copy
/// is dead. `switchTo` saves the active account, then writes the target's credential and
/// `oauthAccount`. Running sessions re-read the Keychain within ~30 s.
///
///     let switcher = ClaudeCodeSwitcher.live(isRunningTests: AppDefaults.isRunningTests())
///     try switcher?.capture(accounts: store.accounts)
///     try await switcher?.switchTo(backend, accounts: store.accounts)
final class ClaudeCodeSwitcher: @unchecked Sendable {
    /// A token this close to expiry, or this little past it, may be refreshing right now.
    static let refreshLead: TimeInterval = 300
    static let refreshGrace: TimeInterval = 60
    static let pollInterval: TimeInterval = 3
    static let maxPolls = 10

    private let slot: ClaudeCodeCredentialSlot
    private let vault: CredentialVaulting
    private let config: ClaudeConfigAccountFile
    private let now: () -> Date
    /// Serializes `capture` and the synchronous part of `switchTo`. Never held across an `await`.
    private let lock = NSLock()
    private let sleep: (TimeInterval) async -> Void

    init(slot: ClaudeCodeCredentialSlot, vault: CredentialVaulting, config: ClaudeConfigAccountFile,
         now: @escaping () -> Date, sleep: @escaping (TimeInterval) async -> Void) {
        self.slot = slot
        self.vault = vault
        self.config = config
        self.now = now
        self.sleep = sleep
    }

    /// The real switcher, or nil under XCTest: the test host is the real app, and its
    /// startup refresh must not read or write the developer's Keychain.
    static func live(isRunningTests: Bool) -> ClaudeCodeSwitcher? {
        guard !isRunningTests else { return nil }
        let keychain = SecurityCLIKeychain()
        return ClaudeCodeSwitcher(
            slot: KeychainClaudeCodeSlot(keychain: keychain, account: NSUserName()),
            vault: KeychainCredentialVault(keychain: keychain),
            config: ClaudeConfigFile(fileURL: ClaudeConfigFile.defaultURL),
            now: Date.init,
            sleep: { seconds in try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000)) })
    }

    // MARK: Capture

    @discardableResult
    func capture(accounts: [Account]) throws -> CaptureResult {
        lock.lock()
        defer { lock.unlock() }
        return try captureUnlocked(accounts: accounts)
    }

    private func captureUnlocked(accounts: [Account]) throws -> CaptureResult {
        guard let accountJSON = try config.readOAuthAccount(),
              let email = OAuthAccountJSON.email(accountJSON),
              let account = Self.match(email, in: accounts) else { return .noActiveAccount }
        guard let oauth = try slot.readOAuth(), !oauth.isBlank else { return .blanked(account.id) }
        if let owner = try otherOwner(of: oauth, activeId: account.id, accounts: accounts) {
            Self.logMismatch(owner: owner.account.id, activeId: account.id)
            return .mismatch(account.id)
        }
        let entry = VaultEntry(oauth: oauth, oauthAccount: accountJSON)
        if try vault.load(account.id) == entry { return .unchanged(account.id) }
        try vault.save(entry, for: account.id)
        return .saved(account.id)
    }

    // MARK: Availability

    func availability(for accounts: [Account]) -> [UUID: SwitchAvailability] {
        let activeEmail = (try? config.readOAuthAccount()).flatMap { OAuthAccountJSON.email($0) }
        let activeId = activeEmail.flatMap { Self.match($0, in: accounts)?.id }
        let activeBlank = (try? slot.readOAuth())?.isBlank ?? true
        var map: [UUID: SwitchAvailability] = [:]
        for account in accounts {
            if account.id == activeId {
                map[account.id] = activeBlank ? .needsLogin : .active
                continue
            }
            guard let entry = try? vault.load(account.id) else {
                map[account.id] = .notCaptured
                continue
            }
            map[account.id] = isExpired(entry) ? .loginExpired : .ready
        }
        return map
    }

    // MARK: Switch

    func switchTo(_ target: Account, accounts: [Account]) async throws {
        // Outside the lock: a capture must stay possible while we wait for the refresh to land.
        await waitForImminentRefresh()
        try performSwitch(to: target, accounts: accounts)
    }

    /// Everything after the wait, synchronous and under the lock so no capture or second
    /// switch interleaves between reading the active account and writing the target.
    private func performSwitch(to target: Account, accounts: [Account]) throws {
        lock.lock()
        defer { lock.unlock() }

        let activeJSON = (try? config.readOAuthAccount()) ?? nil
        let activeEmail = activeJSON.flatMap { OAuthAccountJSON.email($0) }
        let activeAccount = activeEmail.flatMap { Self.match($0, in: accounts) }
        let from = activeAccount.map { "\($0.id)" } ?? "unknown"
        do {
            let switched = try switchLocked(to: target, accounts: accounts, activeJSON: activeJSON,
                                            activeEmail: activeEmail, activeAccount: activeAccount)
            print("[ClaudeCodeSwitcher] switch \(from) -> \(target.id): \(switched ? "switched" : "no-op")")
        } catch let error as SwitchError {
            print("[ClaudeCodeSwitcher] switch \(from) -> \(target.id): \(error.logName)")
            throw error
        }
    }

    /// Returns false for the already-active no-op.
    private func switchLocked(to target: Account, accounts: [Account], activeJSON: Data?,
                              activeEmail: String?, activeAccount: Account?) throws -> Bool {
        if let activeEmail, Self.match(activeEmail, in: [target]) != nil { return false }

        guard let entry = try? vault.load(target.id) else { throw SwitchError.notCaptured }
        guard let vaultEmail = OAuthAccountJSON.email(entry.oauthAccount),
              Self.match(vaultEmail, in: [target]) != nil else {
            print("[ClaudeCodeSwitcher] switch refused: vault copy of \(target.id) names another email")
            throw SwitchError.notCaptured
        }
        guard !isExpired(entry) else { throw SwitchError.loginExpired }

        // One read of the slot: it is both the credential to save and the rollback copy.
        let previous: OAuthCredential?
        do { previous = try slot.readOAuth() } catch { throw SwitchError.keychain }
        let live = previous.flatMap { $0.isBlank ? nil : $0 }
        if live != nil, activeAccount == nil {
            if let activeEmail { throw SwitchError.activeAccountNotInDashboard(email: activeEmail) }
            throw SwitchError.activeAccountUnknown
        }

        do {
            if let live, let activeAccount, let activeJSON {
                if let owner = try otherOwner(of: live, activeId: activeAccount.id, accounts: accounts) {
                    // Not the active account's credential: never file it under that account.
                    // Overwriting it is safe only if its owner's vault already holds it.
                    Self.logMismatch(owner: owner.account.id, activeId: activeAccount.id)
                    guard owner.entry.oauth == live else { throw SwitchError.activeAccountUnknown }
                } else {
                    let current = VaultEntry(oauth: live, oauthAccount: activeJSON)
                    if try vault.load(activeAccount.id) != current { try vault.save(current, for: activeAccount.id) }
                }
            }
            try slot.writeOAuth(entry.oauth)
        } catch let error as SwitchError {
            throw error
        } catch {
            throw SwitchError.keychain
        }

        do {
            try config.writeOAuthAccount(entry.oauthAccount)
        } catch {
            var restored = false
            if let previous { restored = (try? slot.writeOAuth(previous)) != nil }
            throw restored ? SwitchError.config : SwitchError.rollbackFailed
        }

        guard (try? slot.readOAuth())?.json == entry.oauth.json,
              (try? config.readOAuthAccount()) == entry.oauthAccount else {
            throw SwitchError.verifyFailed
        }
        return true
    }

    // MARK: Helpers

    /// A session refreshing the active account during a switch would write its newest
    /// token after the app saved the account, and the switch would then overwrite it.
    /// Wait for that refresh to land. A token further past expiry has no session
    /// refreshing it, so there is nothing to wait for; on timeout, continue anyway.
    private func waitForImminentRefresh() async {
        guard let expiresAt = (try? slot.readOAuth())?.expiresAt else { return }
        let current = now()
        guard expiresAt >= current.addingTimeInterval(-Self.refreshGrace),
              expiresAt <= current.addingTimeInterval(Self.refreshLead) else { return }
        for _ in 0..<Self.maxPolls {
            await sleep(Self.pollInterval)
            if (try? slot.readOAuth())?.expiresAt != expiresAt { return }
        }
    }

    /// The dashboard account, other than `activeId`, whose vault copy has the same
    /// `refreshTokenExpiresAt` as `credential`. That deadline is fixed per grant (spec fact 4),
    /// so a match means `credential` is that account's login, whatever `~/.claude.json` says.
    private func otherOwner(of credential: OAuthCredential, activeId: UUID,
                            accounts: [Account]) throws -> (account: Account, entry: VaultEntry)? {
        guard let deadline = credential.refreshTokenExpiresAt else { return nil }
        for account in accounts where account.id != activeId {
            if let entry = try vault.load(account.id), entry.oauth.refreshTokenExpiresAt == deadline {
                return (account, entry)
            }
        }
        return nil
    }

    private static func logMismatch(owner: UUID, activeId: UUID) {
        print("[ClaudeCodeSwitcher] capture skipped: credential belongs to \(owner), config names \(activeId)")
    }

    private func isExpired(_ entry: VaultEntry) -> Bool {
        guard let deadline = entry.oauth.refreshTokenExpiresAt else { return false }
        return deadline <= now()
    }

    private static func match(_ email: String, in accounts: [Account]) -> Account? {
        accounts.first { $0.email?.caseInsensitiveCompare(email) == .orderedSame }
    }
}
