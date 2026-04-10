import SwiftUI

struct DetectedAccount: Identifiable {
    let id: String // chromeProfilePath (unique per profile)
    let orgId: String
    let chromeProfilePath: String
    let chromeProfileName: String
    let chromeProfileGoogleEmail: String
    let sessionKey: String
    var accountName: String  // Claude account email from org API
    var email: String?       // Claude account email
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
                        addSelectedAccounts()
                        dismissSelf()
                    }
                    .keyboardShortcut(.defaultAction)
                    .disabled(detectedAccounts.filter(\.isSelected).isEmpty)
                }
            }
        }
        .padding(24)
        .frame(width: 520, height: 450)
        .onAppear { scan() }
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
            let results = await Task.detached {
                ChromeCookieService.profilesWithClaudeSessions()
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

            for item in results {
                guard let orgId = item.cookies.orgId,
                      let sessionKey = item.cookies.sessionKey else { continue }

                // Skip profiles already added
                let alreadyAdded = viewModel.accountStore.accounts.contains { $0.chromeProfilePath == item.profile.path }
                if alreadyAdded { continue }

                // Validate session by fetching org info — skip if expired
                guard let orgs = try? await apiService.fetchOrganizations(sessionKey: sessionKey),
                      !orgs.isEmpty else {
                    continue // session expired or invalid, skip this profile
                }

                var email: String? = nil

                // Extract email from personal org name: "{email}'s Organization"
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

                let accountName = email ?? item.profile.displayName

                // Detect plan from usage response
                var plan: AccountPlan? = nil
                if let fullUsage = try? await apiService.fetchFullUsage(orgId: orgId, sessionKey: sessionKey) {
                    plan = fullUsage.planHint
                }

                accounts.append(DetectedAccount(
                    id: item.profile.path,
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
                    self.scanError = "All detected accounts are already added."
                }
            }
        }
    }

    private func addSelectedAccounts() {
        for detected in detectedAccounts where detected.isSelected {
            // Skip if already added
            if viewModel.accountStore.accounts.contains(where: { $0.chromeProfilePath == detected.chromeProfilePath }) {
                continue
            }

            let displayName: String
            if let email = detected.email {
                displayName = email
            } else {
                displayName = detected.accountName
            }

            let chromeLabel = detected.chromeProfileGoogleEmail.isEmpty ? detected.chromeProfileName : detected.chromeProfileGoogleEmail

            let account = Account(
                id: UUID(),
                name: displayName,
                email: detected.email,
                chromeProfilePath: detected.chromeProfilePath,
                chromeProfileName: chromeLabel,
                orgId: detected.orgId,
                plan: detected.plan ?? .pro,
                lastSynced: Date(),
                status: .active
            )

            viewModel.accountStore.addAccount(account)
            viewModel.accountStore.saveSessionKey(detected.sessionKey, for: account.id)
        }

        // Auto-refresh after adding
        Task {
            await viewModel.refreshAll()
        }
    }
}
