import SwiftUI

struct DetectedAccount: Identifiable {
    var id: String { "\(browser.rawValue):\(chromeProfilePath)" }
    let browser: Browser
    let orgId: String
    let chromeProfilePath: String
    let chromeProfileName: String
    let chromeProfileGoogleEmail: String
    let sessionKey: String
    var accountName: String
    var email: String?
    var plan: AccountPlan?
    var isSelected: Bool = true
}

struct SetupView: View {
    @ObservedObject var viewModel: DashboardViewModel
    var onDone: (() -> Void)?
    @Environment(\.dismiss) private var dismiss

    @State private var detectedAccounts: [DetectedAccount] = []
    @State private var isScanning = false
    @State private var scanError: String?
    @State private var selectedBrowser: Browser = .chrome

    var body: some View {
        VStack(spacing: 16) {
            Text("Setup — Sync from Chrome")
                .font(.title2.bold())

            if isScanning {
                VStack(spacing: 8) {
                    ProgressView()
                    Text("Scanning Chrome profiles and detecting accounts...")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            } else if detectedAccounts.isEmpty {
                noProfilesView
            } else {
                accountList
            }

            HStack {
                Button("Cancel") {
                    dismissSelf()
                }
                .keyboardShortcut(.cancelAction)

                Spacer()

                if !detectedAccounts.isEmpty {
                    Button("Add Selected") {
                        Task {
                            await addSelectedAccounts()
                            dismissSelf()
                        }
                    }
                    .keyboardShortcut(.defaultAction)
                    .disabled(detectedAccounts.filter(\.isSelected).isEmpty)
                }
            }
        }
        .padding(24)
        .frame(width: 520, height: 450)
        .onAppear {
            let installed = BrowserCookieService.installedBrowsers()
            selectedBrowser = installed.first ?? .chrome
            scan()
        }
    }

    private func dismissSelf() {
        onDone?()
        dismiss()
    }

    private var noProfilesView: some View {
        VStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle")
                .font(.largeTitle)
                .foregroundStyle(.orange)

            Text(scanError ?? "No Chrome profiles found with active Claude sessions.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)

            Button("Retry Scan") { scan() }
                .padding(.top, 8)
        }
    }

    private var accountList: some View {
        List {
            ForEach($detectedAccounts) { $account in
                HStack {
                    Toggle(isOn: $account.isSelected) {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 6) {
                                Text(account.email ?? account.accountName)
                                    .font(.body.bold())
                                if let plan = account.plan {
                                    Text(plan.rawValue)
                                        .font(.caption2)
                                        .padding(.horizontal, 6)
                                        .padding(.vertical, 2)
                                        .background(plan.badgeColor.opacity(0.15))
                                        .clipShape(Capsule())
                                }
                            }
                            let chromeEmail = account.chromeProfileGoogleEmail
                            Text("Chrome: \(chromeEmail.isEmpty ? account.chromeProfilePath : chromeEmail)")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                        }
                    }
                }
                .padding(.vertical, 4)
            }
        }
    }

    private func scan() {
        isScanning = true
        scanError = nil

        Task {
            let browser = selectedBrowser
            let results = await Task.detached {
                BrowserCookieService.profilesWithClaudeSessions(browser: browser)
            }.value

            if results.isEmpty {
                await MainActor.run {
                    self.detectedAccounts = []
                    self.isScanning = false
                    self.scanError = "No Chrome profiles found with active Claude sessions. Make sure you're logged into claude.ai in your Chrome profiles."
                }
                return
            }

            let apiService = UsageAPIService()
            var accounts: [DetectedAccount] = []
            var duplicateCount = 0
            var validationFailureCount = 0

            for item in results {
                guard let sessionKey = item.cookies.sessionKey else {
                    validationFailureCount += 1
                    continue
                }

                // Validate session by fetching org info — skip if expired
                guard let orgs = try? await apiService.fetchOrganizations(sessionKey: sessionKey),
                      !orgs.isEmpty else {
                    validationFailureCount += 1
                    continue
                }

                // Walk orgs once: prefer the personal org's uuid and email
                // (name pattern "{email}'s Organization").
                var personalOrgId: String? = nil
                var email: String? = nil
                for org in orgs {
                    if org.name.hasSuffix("'s Organization"),
                       let emailPart = org.name.components(separatedBy: "'s Organization").first,
                       emailPart.contains("@") {
                        email = emailPart
                        personalOrgId = org.uuid
                        break
                    }
                }
                if email == nil {
                    email = orgs.compactMap(\.email).first
                }

                // orgId priority: API-derived personal org → cookie's lastActiveOrg →
                // first org from the API. The cookie is unreliable on fresh logins
                // (claude.ai only sets lastActiveOrg after in-org navigation).
                guard let orgId = personalOrgId ?? item.cookies.orgId ?? orgs.first?.uuid,
                      !orgId.isEmpty else {
                    validationFailureCount += 1
                    continue
                }

                let accountName = email ?? item.profile.displayName

                // Account identity = Claude account (email + orgId), NOT the Chrome
                // profile path. Skip only if the same Claude account is already stored.
                // Two different Claude accounts in the same Chrome profile are allowed
                // (the stale one will fail to refresh and can be deleted manually).
                let alreadyStored = viewModel.accountStore.accounts.contains { stored in
                    if stored.orgId == orgId { return true }
                    if let storedEmail = stored.email, let newEmail = email,
                       storedEmail.caseInsensitiveCompare(newEmail) == .orderedSame {
                        return true
                    }
                    return false
                }
                if alreadyStored {
                    duplicateCount += 1
                    continue
                }

                // Detect plan from usage response
                var plan: AccountPlan? = nil
                if let fullUsage = try? await apiService.fetchFullUsage(orgId: orgId, sessionKey: sessionKey) {
                    plan = fullUsage.planHint
                }

                accounts.append(DetectedAccount(
                    browser: item.profile.browser,
                    orgId: orgId,
                    chromeProfilePath: item.profile.path,
                    chromeProfileName: item.profile.displayName,
                    chromeProfileGoogleEmail: item.profile.googleEmail,
                    sessionKey: sessionKey,
                    accountName: accountName,
                    email: email,
                    plan: plan,
                    isSelected: true
                ))
            }

            await MainActor.run {
                self.detectedAccounts = accounts
                self.isScanning = false
                if accounts.isEmpty && !results.isEmpty {
                    if validationFailureCount > 0 && duplicateCount == 0 {
                        self.scanError = "Couldn't validate sessions for the detected Chrome profiles. Make sure you're signed in to claude.ai and try again."
                    } else if validationFailureCount > 0 {
                        self.scanError = "Some accounts are already added; couldn't validate the rest. Make sure you're signed in to claude.ai and try again."
                    } else {
                        self.scanError = "All detected accounts are already added."
                    }
                }
            }
        }
    }

    private func addSelectedAccounts() async {
        for detected in detectedAccounts where detected.isSelected {
            // Belt-and-suspenders: scan() already skips by orgId/email, but in case
            // the store changed between scan and add (or for safety), re-check here.
            let dup = viewModel.accountStore.accounts.contains { stored in
                if stored.orgId == detected.orgId { return true }
                if let storedEmail = stored.email, let newEmail = detected.email,
                   storedEmail.caseInsensitiveCompare(newEmail) == .orderedSame {
                    return true
                }
                return false
            }
            if dup { continue }

            let displayName = detected.email ?? detected.accountName
            let chromeLabel = detected.chromeProfileGoogleEmail.isEmpty ? detected.chromeProfileName : detected.chromeProfileGoogleEmail
            let encryptedSession = CryptoService.encrypt(detected.sessionKey) ?? detected.sessionKey

            let account = Account(
                id: UUID(),
                name: displayName,
                email: detected.email,
                chromeProfilePath: detected.chromeProfilePath,
                chromeProfileName: chromeLabel,
                orgId: detected.orgId,
                sessionKey: encryptedSession,
                browser: detected.browser,
                plan: detected.plan ?? .pro,
                lastSynced: Date(),
                status: .active
            )

            viewModel.accountStore.addAccount(account)
        }

        // Auto-refresh after adding
        Task {
            await viewModel.refreshAll()
        }
    }
}
