import SwiftUI

struct SetupView: View {
    @ObservedObject var viewModel: DashboardViewModel
    @Environment(\.dismiss) private var dismiss

    @State private var detectedProfiles: [(profile: ChromeProfile, cookies: ChromeCookieResult)] = []
    @State private var selectedProfiles: Set<String> = []
    @State private var accountNames: [String: String] = [:]
    @State private var accountPlans: [String: AccountPlan] = [:]
    @State private var isScanning = false
    @State private var scanError: String?

    var body: some View {
        VStack(spacing: 16) {
            Text("Setup — Sync from Chrome")
                .font(.title2.bold())

            if isScanning {
                ProgressView("Scanning Chrome profiles...")
            } else if detectedProfiles.isEmpty {
                noProfilesView
            } else {
                profileList
            }

            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)

                Spacer()

                if !detectedProfiles.isEmpty {
                    Button("Add Selected") {
                        addSelectedAccounts()
                        dismiss()
                    }
                    .keyboardShortcut(.defaultAction)
                    .disabled(selectedProfiles.isEmpty)
                }
            }
        }
        .padding(24)
        .frame(width: 500, height: 300)
        .onAppear { scan() }
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

    private var profileList: some View {
        List {
            ForEach(detectedProfiles, id: \.profile.path) { item in
                HStack {
                    Toggle(isOn: Binding(
                        get: { selectedProfiles.contains(item.profile.path) },
                        set: { isOn in
                            if isOn {
                                selectedProfiles.insert(item.profile.path)
                            } else {
                                selectedProfiles.remove(item.profile.path)
                            }
                        }
                    )) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(item.profile.displayName)
                                .font(.body)
                            Text("Chrome: \(item.profile.path)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }

                    Spacer()

                    if selectedProfiles.contains(item.profile.path) {
                        TextField("Account name", text: Binding(
                            get: { accountNames[item.profile.path] ?? item.profile.displayName },
                            set: { accountNames[item.profile.path] = $0 }
                        ))
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 140)

                        Picker("", selection: Binding(
                            get: { accountPlans[item.profile.path] ?? .max200 },
                            set: { accountPlans[item.profile.path] = $0 }
                        )) {
                            ForEach(AccountPlan.allCases, id: \.self) { plan in
                                Text(plan.rawValue)
                            }
                        }
                        .frame(width: 70)
                    }
                }
                .padding(.vertical, 4)
            }
        }
    }

    private func scan() {
        isScanning = true
        scanError = nil

        DispatchQueue.global(qos: .userInitiated).async {
            let results = ChromeCookieService.profilesWithClaudeSessions()

            DispatchQueue.main.async {
                self.detectedProfiles = results
                self.isScanning = false

                if results.isEmpty {
                    self.scanError = "No Chrome profiles found with active Claude sessions. Make sure you're logged into claude.ai in your Chrome profiles."
                }
            }
        }
    }

    private func addSelectedAccounts() {
        for item in detectedProfiles where selectedProfiles.contains(item.profile.path) {
            if viewModel.accountStore.accounts.contains(where: { $0.chromeProfilePath == item.profile.path }) {
                continue
            }

            let name = accountNames[item.profile.path] ?? item.profile.displayName
            let plan = accountPlans[item.profile.path] ?? .max200

            let account = Account(
                id: UUID(),
                name: name,
                chromeProfilePath: item.profile.path,
                orgId: item.cookies.orgId,
                plan: plan,
                lastSynced: Date(),
                status: .active
            )

            viewModel.accountStore.addAccount(account)

            if let sessionKey = item.cookies.sessionKey {
                viewModel.accountStore.saveSessionKey(sessionKey, for: account.id)
            }
        }
    }
}
