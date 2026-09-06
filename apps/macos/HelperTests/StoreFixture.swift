import Foundation

/// Builds and inspects a throwaway UserDefaults suite for the helper's account
/// store. Every suite name carries a UUID so two tests never share one, and
/// `destroy` unlinks the suite's backing plist file, so a run leaves nothing
/// behind on disk in ~/Library/Preferences.
enum StoreFixture {

    static let storageKey = "claude-dashboard.accounts"

    /// Every fixture suite name starts with this. `destroy` only ever unlinks
    /// a plist whose suite name has this prefix -- the guard that makes an
    /// unconditional filesystem delete safe to keep in the repo at all.
    static let testSuitePrefix = "com.claude-dashboard.test."

    static func makeSuiteName() -> String {
        "\(testSuitePrefix)\(UUID().uuidString)"
    }

    static func seed(_ accounts: [Account], intoSuite suite: String) {
        guard let defaults = UserDefaults(suiteName: suite),
              let data = try? JSONEncoder().encode(accounts) else {
            fatalError("could not seed the fixture suite")
        }
        defaults.set(data, forKey: storageKey)
    }

    /// Reads what the *child process* wrote. `CFPreferencesAppSynchronize`
    /// first: preferences are served by cfprefsd and cached per process, so a
    /// parent that has already read this suite can otherwise serve a stale
    /// snapshot after the child writes to it.
    static func read(fromSuite suite: String) -> [Account] {
        CFPreferencesAppSynchronize(suite as CFString)
        guard let defaults = UserDefaults(suiteName: suite),
              let data = defaults.data(forKey: storageKey),
              let accounts = try? JSONDecoder().decode([Account].self, from: data) else {
            return []
        }
        return accounts
    }

    /// Unlinks the suite's backing plist at
    /// ~/Library/Preferences/<suite>.plist. Safe to call twice, and safe for a
    /// suite that was never written to: a missing file is not an error.
    ///
    /// Deliberately does **not** call `removePersistentDomain`: isolated
    /// probing (see task-3-report.md, fix round 1) showed `removePersistentDomain`
    /// is itself what leaves the 42-byte empty plist behind when the suite had
    /// dirty data -- calling it schedules cfprefsd's own async rewrite of the
    /// (now empty) domain to disk, which can land *after* this function's own
    /// delete and silently recreate the file. `CFPreferencesAppSynchronize`
    /// alone -- forcing out whatever `seed`/`saveAccounts` last wrote, with no
    /// remove in between to re-dirty the domain -- followed by the delete,
    /// reproduced zero leaks over 9 isolated runs where remove-then-delete
    /// reliably leaked. Skipping `removePersistentDomain` leaves the suite's
    /// (now-orphaned) entry in cfprefsd's in-memory registry until the daemon
    /// itself reclaims it, but nothing in this process or fixture ever reads
    /// that suite name again -- every suite is a fresh UUID -- so the only
    /// cost is a harmless, unread cache entry, not a leaked file.
    ///
    /// The delete only runs when `suite` carries `testSuitePrefix` -- never
    /// for a suite this fixture could not itself have created. That guard is
    /// what makes an unconditional filesystem delete safe to keep here at all.
    static func destroy(suite: String) {
        guard suite.hasPrefix(testSuitePrefix) else { return }
        CFPreferencesAppSynchronize(suite as CFString)
        let plistPath = NSHomeDirectory() + "/Library/Preferences/\(suite).plist"
        try? FileManager.default.removeItem(atPath: plistPath)
    }

    /// A stored account with every field set explicitly. Defaults here are test
    /// convenience only; production code has none (see `Account.source`).
    static func account(
        id: UUID = UUID(),
        name: String,
        email: String? = nil,
        orgId: String?,
        accountUuid: String? = "acct-fixture",
        sessionKey: String? = "sk-fake-stored-key",
        plan: AccountPlan = .pro,
        status: AccountStatus = .active,
        source: AccountSource = .browser
    ) -> Account {
        Account(
            id: id,
            name: name,
            email: email ?? name,
            chromeProfilePath: "Default",
            chromeProfileName: nil,
            orgId: orgId,
            accountUuid: accountUuid,
            sessionKey: sessionKey,
            browser: .chrome,
            plan: plan,
            lastSynced: Date(timeIntervalSince1970: 1_000_000),
            status: status,
            source: source
        )
    }
}
