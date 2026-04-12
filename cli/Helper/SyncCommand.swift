import Foundation

enum SyncCommand {

    static func run() -> Int32 {
        fputs("Scanning Chrome profiles...\n", stderr)

        let results = ChromeCookieService.profilesWithClaudeSessions()

        if results.isEmpty {
            fputs("No Chrome profiles found with active Claude sessions.\n", stderr)
            fputs("Make sure you're logged into claude.ai in Chrome.\n", stderr)
            return 1
        }

        fputs("Found \(results.count) profile(s) with Claude sessions. Validating...\n", stderr)

        var existingAccounts = HelperAccountStore.loadAccounts()
        var addedCount = 0

        let semaphore = DispatchSemaphore(value: 0)

        Task {
            let apiService = UsageAPIService()

            for item in results {
                guard let orgId = item.cookies.orgId,
                      let sessionKey = item.cookies.sessionKey else { continue }

                // Skip if already added
                if existingAccounts.contains(where: { $0.chromeProfilePath == item.profile.path }) {
                    fputs("  Skipping \(item.profile.displayName) (already added)\n", stderr)
                    continue
                }

                // Validate session
                guard let orgs = try? await apiService.fetchOrganizations(sessionKey: sessionKey),
                      !orgs.isEmpty else {
                    fputs("  Skipping \(item.profile.displayName) (session expired)\n", stderr)
                    continue
                }

                // Extract email
                var email: String? = nil
                for org in orgs {
                    if org.name.hasSuffix("'s Organization"),
                       let emailPart = org.name.components(separatedBy: "'s Organization").first,
                       emailPart.contains("@") {
                        email = emailPart
                        break
                    }
                }
                if email == nil {
                    email = orgs.compactMap(\.email).first
                }

                // Detect plan
                var plan: AccountPlan = .pro
                if let fullUsage = try? await apiService.fetchFullUsage(orgId: orgId, sessionKey: sessionKey) {
                    plan = fullUsage.planHint ?? .pro
                }

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
                    sessionKey: CryptoService.encrypt(sessionKey) ?? sessionKey,
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
