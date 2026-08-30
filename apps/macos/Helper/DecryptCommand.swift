import Foundation

enum DecryptCommand {

    struct DecryptedAccount: Encodable {
        let name: String
        let email: String?
        let orgId: String?
        let sessionKey: String?
        let plan: String
        let status: String
    }

    static func run() -> Int32 {
        let accounts = HelperAccountStore.loadAccounts()

        if accounts.isEmpty {
            fputs("No accounts found. Run: claude-dashboard-cli sync\n", stderr)
            return 1
        }

        let decrypted: [DecryptedAccount] = accounts.compactMap { account in
            guard account.status == .active,
                  account.orgId != nil else { return nil }

            var plainKey: String? = nil
            if let encrypted = account.sessionKey {
                plainKey = CryptoService.decrypt(encrypted) ?? encrypted
            }

            return DecryptedAccount(
                name: account.name,
                email: account.email,
                orgId: account.orgId,
                sessionKey: plainKey,
                plan: account.plan.rawValue,
                status: account.status.rawValue
            )
        }

        if decrypted.isEmpty {
            fputs("No active accounts with session keys found.\n", stderr)
            return 1
        }

        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(decrypted),
              let json = String(data: data, encoding: .utf8) else {
            fputs("Failed to encode accounts.\n", stderr)
            return 1
        }

        print(json)
        return 0
    }
}
