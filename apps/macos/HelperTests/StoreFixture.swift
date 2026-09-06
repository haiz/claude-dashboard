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

    /// Removes the suite's backing plist at
    /// ~/Library/Preferences/<suite>.plist AND tells cfprefsd to forget the
    /// domain. Safe to call twice, and safe for a suite that was never
    /// written to: "domain does not exist" / a missing file are not errors,
    /// they are the common case for a suite this fixture only ever read.
    ///
    /// Deliberately does **not** call `UserDefaults.removePersistentDomain`:
    /// isolated probing (see task-3-report.md, fix round 1) showed it is
    /// itself what leaves a 42-byte empty plist behind when the suite had
    /// dirty data -- calling it schedules cfprefsd's own async rewrite of the
    /// (now empty) domain to disk, which can land *after* this function's own
    /// delete and silently recreate the file. `CFPreferencesAppSynchronize`
    /// alone, followed by a plain file delete with no `removePersistentDomain`
    /// in between, reproduced zero leaks over 9 isolated runs there.
    ///
    /// That fix was correct but incomplete -- it was only ever measured
    /// against an **in-process** writer. Task 9 found the missing half: when
    /// a **child process** wrote the domain (`add-key`'s add and repair
    /// paths), cfprefsd still held it after this function returned, and
    /// flushed it back to disk up to ~6s later even though the file had
    /// already been deleted here -- confirmed by a red baseline of 10/10
    /// full-class runs, each checked a full 10s after the run (see
    /// task-9-report.md fix round 1). The old theory that skipping
    /// `removePersistentDomain` leaves only "a harmless, unread cache entry,
    /// not a leaked file" was wrong for this direction: the daemon does not
    /// just cache the domain, it can still re-persist it.
    ///
    /// The fix is `/usr/bin/defaults delete`, run as a **separate process**:
    /// unlike `removePersistentDomain` (an in-process call against this same
    /// address space's CFPreferences view), it talks to cfprefsd's domain
    /// registry directly and makes the daemon itself forget the domain --
    /// exactly the layer the delayed re-flush above came from. It still
    /// leaves the same 42-byte empty-domain stub `removePersistentDomain`
    /// does (measured directly), so the explicit unlink below is still
    /// required. The short retry loop after it is a bounded (<=100ms) guard
    /// against that stub's own write landing a beat late, not a wait for the
    /// multi-second daemon flush -- `defaults delete` is what eliminates
    /// that, verified over 24/24 clean runs (task-9-report.md fix round 1).
    ///
    /// The domain removal and file delete only run when `suite` carries
    /// `testSuitePrefix` -- never for a suite this fixture could not itself
    /// have created. That guard is what makes an unconditional domain removal
    /// safe to keep here at all.
    static func destroy(suite: String) {
        guard suite.hasPrefix(testSuitePrefix) else { return }
        CFPreferencesAppSynchronize(suite as CFString)

        // Talks to cfprefsd directly, unlike an in-process
        // `removePersistentDomain` -- see the doc comment above for why that
        // distinction is the whole fix. "Domain does not exist" exits
        // non-zero; that failure (like every other one here) is discarded on
        // purpose, matching the existing "a missing file is not an error"
        // contract of this function.
        let forgetDomain = Process()
        forgetDomain.executableURL = URL(fileURLWithPath: "/usr/bin/defaults")
        forgetDomain.arguments = ["delete", suite]
        forgetDomain.standardOutput = FileHandle.nullDevice
        forgetDomain.standardError = FileHandle.nullDevice
        // `waitUntilExit()` is only valid after a successful `run()`; guarding
        // it is what keeps a missing/unlaunchable binary a no-op here rather
        // than a crash in cleanup code (the class of failure task-7-report.md
        // hit with a force-unwrap: a crash skips tearDown for everything after
        // it, which is strictly worse than the leak this function exists to
        // prevent).
        if (try? forgetDomain.run()) != nil {
            forgetDomain.waitUntilExit()
        }

        let plistPath = NSHomeDirectory() + "/Library/Preferences/\(suite).plist"
        try? FileManager.default.removeItem(atPath: plistPath)

        // Belt-and-braces only: `defaults delete` above is the real
        // mechanism, and it is what stops the multi-second daemon re-flush.
        // This loop is a bounded (<=100ms) guard against ITS OWN empty-stub
        // write (see above) landing a beat after our unlink -- never a wait
        // for the daemon flush the real fix already eliminated.
        for _ in 0..<5 {
            guard FileManager.default.fileExists(atPath: plistPath) else { return }
            usleep(20_000)
            try? FileManager.default.removeItem(atPath: plistPath)
        }
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
