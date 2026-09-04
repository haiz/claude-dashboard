import Foundation

/// `claude-dashboard-helper add-key` — adds or repairs one account from a
/// session key read on stdin. See `contract/helper-cli.md` "add-key" for the
/// stderr lines and exit codes, which the Linux helper matches exactly.
///
/// The key never reaches stderr, on any branch.
enum AddKeyCommand {

    static func run() -> Int32 {
        let raw = String(decoding: FileHandle.standardInput.readDataToEndOfFile(), as: UTF8.self)
        guard let sessionKey = ManualKeyInput.trimmedKey(from: raw) else {
            fputs("No session key on stdin.\n", stderr)
            return 1
        }

        var accounts = HelperAccountStore.loadAccounts()
        var code: Int32 = 0
        let semaphore = DispatchSemaphore(value: 0)

        Task {
            defer { semaphore.signal() }
            let apiService = UsageAPIService()

            guard let candidate = await SessionCandidate.validate(
                sessionKey: sessionKey,
                against: accounts.map(StoredIdentity.init),
                apiService: apiService
            ) else {
                fputs("Session key not accepted (expired or invalid).\n", stderr)
                code = 1
                return
            }

            let target = candidate.duplicateIndex.map { StoredManualTarget(accounts[$0]) }
            let decision = ManualKey.decision(
                stored: target,
                fetchedUuid: candidate.identity.uuid,
                fetchedEmail: candidate.identity.email,
                memberships: candidate.identity.memberships)

            switch decision {
            case .rejectNoChatOrg:
                fputs("No organization with chat access.\n", stderr)
                code = 1

            case .add(let orgId):
                let email = candidate.identity.email
                let name = email ?? "Account \(candidate.identity.uuid.prefix(8))"
                let plan = candidate.orgs?.first(where: { $0.uuid == orgId })?.planHint ?? .pro
                accounts.append(Account(
                    id: UUID(),
                    name: name,
                    email: email,
                    chromeProfilePath: "",
                    chromeProfileName: nil,
                    orgId: orgId,
                    accountUuid: candidate.identity.uuid,
                    sessionKey: CryptoService.encrypt(sessionKey) ?? sessionKey,
                    browser: .chrome,
                    plan: plan,
                    lastSynced: Date(),
                    status: .active,
                    source: .manual
                ))
                HelperAccountStore.saveAccounts(accounts)
                fputs("Added: \(name) (\(plan.rawValue))\n", stderr)

            case .repair(let writes):
                guard let index = candidate.duplicateIndex else { return }
                var updated = accounts[index]
                let oldPlan = updated.plan
                updated.sessionKey = CryptoService.encrypt(sessionKey) ?? sessionKey
                updated.status = .active
                updated.lastSynced = Date()
                if let orgId = writes.orgId { updated.orgId = orgId }
                if let uuid = writes.accountUuid { updated.accountUuid = uuid }
                if let email = writes.email { updated.email = email }
                // Matched against the orgId as it stands after those writes.
                if let plan = UsageAPIService.refreshedPlan(for: updated, orgs: candidate.orgs) {
                    updated.plan = plan
                }
                accounts[index] = updated
                HelperAccountStore.saveAccounts(accounts)

                let name = updated.email ?? updated.name
                fputs("Updated key: \(name)\n", stderr)
                if updated.plan != oldPlan {
                    fputs("Updated plan: \(name) (\(oldPlan.rawValue) -> \(updated.plan.rawValue))\n", stderr)
                }
                if updated.orgId == nil {
                    fputs("Warning: no organization with chat access; usage will not update.\n", stderr)
                }
            }
        }

        semaphore.wait()
        return code
    }
}
