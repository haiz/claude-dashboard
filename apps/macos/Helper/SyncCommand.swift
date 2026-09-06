import Foundation

/// Carries the exit code out of the detached `Task` in `SyncCommand.run(env:)`.
private final class ExitCodeBox: @unchecked Sendable {
    var code: Int32 = 0
}

enum SyncCommand {

    /// Everything `sync` touches outside its own logic. No default value on
    /// purpose: a caller that forgot to pass one would scan real cookie
    /// databases and rewrite the real account store, which is exactly the
    /// failure a test must never be able to cause by omission.
    struct Environment {
        var candidates: () -> [(profile: BrowserProfile, cookies: ChromeCookieResult)]
        var apiService: UsageAPIService
        var loadAccounts: () -> [Account]
        var saveAccounts: ([Account]) -> Void
        /// Receives each line without its newline; `live` appends it.
        var log: (String) -> Void

        static let live = Environment(
            candidates: {
                BrowserCookieService.installedBrowsers().flatMap { browser in
                    BrowserCookieService.profilesWithClaudeSessions(browser: browser)
                }
            },
            apiService: UsageAPIService(),
            loadAccounts: { HelperAccountStore.loadAccounts() },
            saveAccounts: { HelperAccountStore.saveAccounts($0) },
            log: { fputs($0 + "\n", stderr) }
        )
    }

    /// The synchronous entry point `main.swift` needs. Tests drive `runAsync`
    /// instead: blocking a test thread on this semaphore while `URLSession`
    /// completes elsewhere is the shape of the repo's known flaky test.
    static func run(env: Environment) -> Int32 {
        let semaphore = DispatchSemaphore(value: 0)
        let box = ExitCodeBox()
        Task {
            box.code = await runAsync(env: env)
            semaphore.signal()
        }
        semaphore.wait()
        return box.code
    }

    static func runAsync(env: Environment) async -> Int32 {
        env.log("Scanning installed browsers for Claude sessions...")

        let results = env.candidates()

        if results.isEmpty {
            env.log("No browser profiles found with active Claude sessions.")
            env.log("Make sure you're logged into claude.ai in a supported browser.")
            return 1
        }

        env.log("Found \(results.count) profile(s) with Claude sessions. Validating...")

        var existingAccounts = env.loadAccounts()
        var addedCount = 0

        for item in results {
            guard let sessionKey = item.cookies.sessionKey else { continue }

            // Identity from /api/account. A failure is an unusable session.
            guard let info = try? await env.apiService.fetchAccount(sessionKey: sessionKey) else {
                env.log("  Skipping \(item.profile.displayName) (session expired)")
                continue
            }

            let email = info.email

            // Same dedupe rule as the app: the Claude account, not the
            // browser profile. See contract/cases/dedupe.json.
            if let index = AccountIdentity.duplicateIndex(
                candidateUuid: info.uuid,
                candidateEmail: email,
                against: existingAccounts.map(StoredIdentity.init)
            ) {
                env.log("  Skipping \(item.profile.displayName) (already added)")
                // Skipped for *adding* only — the stored plan tier is still
                // refreshed, so a tier that fell back to `.pro` because
                // /api/organizations was down at add time heals here.
                let stored = existingAccounts[index]
                if let newPlan = await Self.refreshedStoredPlan(
                    for: stored, sessionKey: sessionKey, apiService: env.apiService) {
                    env.log("  Updated plan: \(item.profile.displayName) "
                        + "(\(stored.plan.rawValue) -> \(newPlan.rawValue))")
                    existingAccounts[index].plan = newPlan
                }
                continue
            }

            guard let orgId = AccountIdentity.resolveOrgId(
                lastActiveOrg: item.cookies.orgId, memberships: info.memberships) else {
                env.log("  Skipping \(item.profile.displayName) (no usable org)")
                continue
            }

            let plan = (try? await env.apiService.fetchOrganizations(sessionKey: sessionKey))?
                .first(where: { $0.uuid == orgId })?.planHint ?? .pro

            let displayName = email ?? item.profile.displayName
            let chromeLabel = item.profile.googleEmail.isEmpty
                ? item.profile.displayName
                : item.profile.googleEmail

            let account = Account(
                id: UUID(),
                name: displayName,
                email: email,
                chromeProfilePath: item.profile.path,
                chromeProfileName: chromeLabel,
                orgId: orgId,
                accountUuid: info.uuid,
                sessionKey: CryptoService.encrypt(sessionKey) ?? sessionKey,
                browser: item.profile.browser,
                plan: plan,
                lastSynced: Date(),
                status: .active,
                source: .browser
            )

            existingAccounts.append(account)
            addedCount += 1
            env.log("  Added: \(displayName) (\(plan.rawValue))")
        }

        env.saveAccounts(existingAccounts)

        if addedCount == 0 {
            env.log("No new accounts to add (all already synced).")
        } else {
            env.log("Synced \(addedCount) account(s) successfully.")
        }

        return 0
    }

    /// The plan to write for an account `sync` just skipped as a duplicate, or
    /// nil to leave it as it is — the CLI's counterpart to the GUI's
    /// per-refresh plan update (`DashboardViewModel.refreshAll`). Mirrors
    /// `refresh_stored_plan` in `apps/linux/helper/src/sync.rs`;
    /// `contract/helper-cli.md` "sync" specifies the extra stderr line.
    ///
    /// Only the plan is ever written: not `sessionKey`, not `lastSynced`, not
    /// `status`. An account with no `orgId` has no org to match against and is
    /// left alone.
    private static func refreshedStoredPlan(
        for account: Account,
        sessionKey: String,
        apiService: UsageAPIService
    ) async -> AccountPlan? {
        // Deliberately no `?? .pro`: unlike the add path, orgs that do not
        // resolve must leave the stored tier alone (rule 1 of
        // contract/cases/plan-refresh.json).
        let orgs = try? await apiService.fetchOrganizations(sessionKey: sessionKey)

        return UsageAPIService.refreshedPlan(for: account, orgs: orgs)
    }
}
