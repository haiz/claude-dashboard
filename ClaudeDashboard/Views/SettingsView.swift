import SwiftUI

struct SettingsView: View {
    @ObservedObject var viewModel: DashboardViewModel
    @EnvironmentObject var updateViewModel: UpdateViewModel
    @Environment(\.dismiss) private var dismiss
    @State private var showingSetup = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Settings")
                    .font(.title2.bold())
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
            .padding()

            Divider()

            List {
                Section("Accounts") {
                    ForEach(viewModel.accountStore.accounts) { account in
                        accountRow(account)
                    }
                }

                Section("Auto Refresh") {
                    Toggle("Enable auto refresh", isOn: $viewModel.autoRefreshEnabled)

                    if viewModel.autoRefreshEnabled {
                        Stepper("Every \(viewModel.autoRefreshMinutes) min", value: $viewModel.autoRefreshMinutes, in: 1...60)
                    }
                }

                Section("Updates") {
                    Toggle("Auto-update daily", isOn: $updateViewModel.autoUpdateEnabled)

                    if updateViewModel.autoUpdateEnabled {
                        Text("Checks GitHub once a day and installs new releases automatically.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Current: v\(AppVersion.string)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            if let latest = updateViewModel.latestVersion {
                                Text("Latest: v\(latest)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            if !updateViewModel.state.statusLabel.isEmpty {
                                Text(updateViewModel.state.statusLabel)
                                    .font(.caption)
                                    .foregroundStyle(
                                        updateStatusColor(updateViewModel.state)
                                    )
                            }
                        }
                        Spacer()
                        #if DEBUG
                        Button("Check for Updates") {}
                            .disabled(true)
                            .help("Updates only run in release builds")
                        #else
                        Button(updateViewModel.state.isWorking ? "Working…" : "Check for Updates") {
                            Task { await updateViewModel.checkNow(autoInstall: true) }
                        }
                        .disabled(updateViewModel.state.isWorking)
                        #endif
                    }
                }
            }

            Divider()

            HStack {
                Button(action: { showingSetup = true }) {
                    Label("Add from Chrome", systemImage: "plus.circle")
                }

                Spacer()

                Button("Re-sync All from Chrome") {
                    Task {
                        for account in viewModel.accountStore.accounts {
                            await viewModel.resyncAccount(account.id)
                        }
                    }
                }
            }
            .padding()

            Divider()

            HStack {
                Spacer()
                Text("Claude Dashboard v\(AppVersion.string)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
            }
            .padding(.vertical, 6)
        }
        .frame(width: 500, height: 400)
        .sheet(isPresented: $showingSetup) {
            SetupView(viewModel: viewModel) {
                showingSetup = false
            }
        }
    }

    private func updateStatusColor(_ state: UpdateViewModel.State) -> Color {
        switch state {
        case .error: return .red
        case .upToDate: return .green
        default: return .secondary
        }
    }

    private func accountRow(_ account: Account) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(account.name)
                    .font(.body)
                HStack(spacing: 4) {
                    Text(account.plan.rawValue)
                        .font(.caption)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(.secondary.opacity(0.15))
                        .clipShape(Capsule())

                    Text(account.chromeProfilePath)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            if account.status == .expired {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
            }

            Button(action: {
                viewModel.accountStore.removeAccount(id: account.id)
            }) {
                Image(systemName: "trash")
            }
            .buttonStyle(HoverableButtonStyle(horizontalPadding: 6, verticalPadding: 4))
            .foregroundStyle(.red)
        }
        .padding(.vertical, 2)
    }
}
