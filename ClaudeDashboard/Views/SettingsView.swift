import SwiftUI

struct SettingsView: View {
    @ObservedObject var viewModel: DashboardViewModel
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
            .buttonStyle(.borderless)
            .foregroundStyle(.red)
        }
        .padding(.vertical, 2)
    }
}
