import SwiftUI

/// Settings > Accounts: the account list with delete buttons, plus Add Account
/// and Re-sync All under the group, as System Settings places list actions.
///
///     AccountsSettingsPane(viewModel: vm, onAddAccount: { showingSetup = true })
struct AccountsSettingsPane: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onAddAccount: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "Accounts")
            Form {
                Section {
                    if viewModel.accountStore.accounts.isEmpty {
                        Text("No accounts yet.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(viewModel.accountStore.accounts) { account in
                        accountRow(account)
                    }
                } footer: {
                    HStack {
                        Spacer()
                        Button("Add Account\u{2026}", action: onAddAccount)
                            .help("Add a Claude account from your browser or a pasted session key")
                        Button(viewModel.resyncAllProgress.map { "Re-syncing\u{2026} (\($0.done)/\($0.total))" } ?? "Re-sync All") {
                            Task { await viewModel.resyncAll() }
                        }
                        .disabled(viewModel.resyncAllProgress != nil || viewModel.accountStore.accounts.isEmpty)
                        .help("Read fresh session keys from the browser for every account")
                    }
                }
            }
            .formStyle(.grouped)
        }
    }

    private func accountRow(_ account: Account) -> some View {
        HStack(spacing: 10) {
            AccountAvatar(account: account, size: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(account.name)
                HStack(spacing: 4) {
                    Text(account.plan.rawValue)
                        .font(.caption)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(.secondary.opacity(0.15))
                        .clipShape(Capsule())
                    Text(account.source == .manual ? "Pasted key" : account.chromeProfilePath)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            if account.status == .expired {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .help("Session expired")
            }
            Button {
                viewModel.accountStore.removeAccount(id: account.id)
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(HoverableButtonStyle(horizontalPadding: 6, verticalPadding: 4))
            .foregroundStyle(.red)
            .help("Remove account")
        }
        .padding(.vertical, 2)
    }
}
