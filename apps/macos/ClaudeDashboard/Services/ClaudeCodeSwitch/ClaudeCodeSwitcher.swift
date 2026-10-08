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
    case notCaptured
    case loginExpired
    /// Claude Code is signed in as an account the dashboard does not know; switching
    /// would discard its refresh token.
    case activeAccountNotInDashboard(email: String)
    case keychain
    /// `~/.claude.json` could not be written; the previous credential was restored.
    case config
    case verifyFailed
}

enum CaptureResult: Equatable {
    case noActiveAccount
    case blanked(UUID)
    case unchanged(UUID)
    case saved(UUID)
}

/// Keeps every account's Claude Code login alive and swaps it into `~/.claude`.
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
        guard let accountJSON = try config.readOAuthAccount(),
              let email = OAuthAccountJSON.email(accountJSON),
              let account = Self.match(email, in: accounts) else { return .noActiveAccount }
        guard let oauth = try slot.readOAuth(), !oauth.isBlank else { return .blanked(account.id) }
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
        let activeEmail = (try? config.readOAuthAccount()).flatMap { OAuthAccountJSON.email($0) }
        if let activeEmail, Self.match(activeEmail, in: [target]) != nil { return }

        guard let entry = try? vault.load(target.id) else { throw SwitchError.notCaptured }
        guard !isExpired(entry) else { throw SwitchError.loginExpired }

        if let activeEmail, Self.match(activeEmail, in: accounts) == nil,
           let current = try? slot.readOAuth(), !current.isBlank {
            throw SwitchError.activeAccountNotInDashboard(email: activeEmail)
        }

        await waitForImminentRefresh()

        let previous: OAuthCredential?
        do {
            try capture(accounts: accounts)
            previous = try slot.readOAuth()
            try slot.writeOAuth(entry.oauth)
        } catch {
            print("[ClaudeCodeSwitcher] keychain step failed switching to \(target.id): \(error)")
            throw SwitchError.keychain
        }

        do {
            try config.writeOAuthAccount(entry.oauthAccount)
        } catch {
            if let previous { try? slot.writeOAuth(previous) }
            print("[ClaudeCodeSwitcher] config write failed, credential rolled back: \(error)")
            throw SwitchError.config
        }

        guard (try? slot.readOAuth())?.json == entry.oauth.json,
              (try? config.readOAuthAccount()) == entry.oauthAccount else {
            throw SwitchError.verifyFailed
        }
        print("[ClaudeCodeSwitcher] switched to \(target.id)")
    }

    // MARK: Helpers

    /// A session refreshing the active account during a switch would write its newest
    /// token after the app saved the account, and the switch would then overwrite it.
    /// Wait for that refresh to land. A token further past expiry has no session
    /// refreshing it, so there is nothing to wait for; on timeout, continue anyway.
    private func waitForImminentRefresh() async {
        guard let expiresAt = (try? slot.readOAuth())?.expiresAt else { return }
        let current = now()
        guard expiresAt > current.addingTimeInterval(-Self.refreshGrace),
              expiresAt < current.addingTimeInterval(Self.refreshLead) else { return }
        for _ in 0..<Self.maxPolls {
            await sleep(Self.pollInterval)
            if (try? slot.readOAuth())?.expiresAt != expiresAt { return }
        }
    }

    private func isExpired(_ entry: VaultEntry) -> Bool {
        guard let deadline = entry.oauth.refreshTokenExpiresAt else { return false }
        return deadline <= now()
    }

    private static func match(_ email: String, in accounts: [Account]) -> Account? {
        accounts.first { $0.email?.caseInsensitiveCompare(email) == .orderedSame }
    }
}
