import Foundation

enum SyncCommand {

    static func run() -> Int32 {
        fputs("Scanning installed browsers for Claude sessions...\n", stderr)

        let installed = BrowserCookieService.installedBrowsers()
        let results = installed.flatMap { browser in
            BrowserCookieService.profilesWithClaudeSessions(browser: browser)
        }

        if results.isEmpty {
            fputs("No browser profiles found with active Claude sessions.\n", stderr)
            fputs("Make sure you're logged into claude.ai in a supported browser.\n", stderr)
            return 1
        }

        fputs("Found \(results.count) profile(s) with Claude sessions. Validating...\n", stderr)

        var existingAccounts = HelperAccountStore.loadAccounts()
        var addedCount = 0

        let semaphore = DispatchSemaphore(value: 0)

        Task {
            let apiService = UsageAPIService()

            for item in results {
                guard let sessionKey = item.cookies.sessionKey else { continue }

                // Identity from /api/account. A failure is an unusable session.
                guard let info = try? await apiService.fetchAccount(sessionKey: sessionKey) else {
                    fputs("  Skipping \(item.profile.displayName) (session expired)\n", stderr)
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
                    fputs("  Skipping \(item.profile.displayName) (already added)\n", stderr)
                    // Skipped for *adding* only — the stored plan tier is still
                    // refreshed, so a tier that fell back to `.pro` because
                    // /api/organizations was down at add time heals here.
                    let stored = existingAccounts[index]
                    if let newPlan = await Self.refreshedStoredPlan(
                        for: stored, sessionKey: sessionKey, apiService: apiService) {
                        fputs("  Updated plan: \(item.profile.displayName) "
                            + "(\(stored.plan.rawValue) -> \(newPlan.rawValue))\n", stderr)
                        existingAccounts[index].plan = newPlan
                    }
                    continue
                }

                guard let orgId = AccountIdentity.resolveOrgId(
                    lastActiveOrg: item.cookies.orgId, memberships: info.memberships) else {
                    fputs("  Skipping \(item.profile.displayName) (no usable org)\n", stderr)
                    continue
                }

                let plan = (try? await apiService.fetchOrganizations(sessionKey: sessionKey))?
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
                fputs("  Added: \(displayName) (\(plan.rawValue))\n", stderr)
            }

            HelperAccountStore.saveAccounts(existingAccounts)
            semaphore.signal()
        }

        semaphore.wait()

        if addedCount == 0 {
            fputs("No new accounts to add (all already synced).\n", stderr)
        } else {
            fputs("Synced \(addedCount) account(s) successfully.\n", stderr)
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
