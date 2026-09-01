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
                if AccountIdentity.isDuplicate(
                    candidateUuid: info.uuid,
                    candidateEmail: email,
                    against: existingAccounts.map(StoredIdentity.init)
                ) {
                    fputs("  Skipping \(item.profile.displayName) (already added)\n", stderr)
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
                    status: .active
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
}
