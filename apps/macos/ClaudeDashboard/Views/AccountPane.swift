import SwiftUI

/// Main-window pane for one account, laid out like the Apple ID page in System
/// Settings: identity header, the usage gauges, the usage chart, then actions.
/// Give it `.id(state.id)` so switching accounts rebuilds the chart state.
///
///     AccountPane(dashboardViewModel: vm, state: state,
///                 onRunCommand: { runCommandAccount = state.account })
///         .id(state.id)
struct AccountPane: View {
    @ObservedObject var dashboardViewModel: DashboardViewModel
    let state: AccountUsageState
    let onRunCommand: () -> Void

    @StateObject private var chartViewModel: AccountDetailViewModel
    @State private var confirmingRemove = false

    init(dashboardViewModel: DashboardViewModel, state: AccountUsageState, onRunCommand: @escaping () -> Void) {
        self.dashboardViewModel = dashboardViewModel
        self.state = state
        self.onRunCommand = onRunCommand
        _chartViewModel = StateObject(wrappedValue: AccountDetailViewModel(
            accountId: state.id,
            accountName: state.account.name,
            accountPlan: state.account.plan,
            logStore: dashboardViewModel.logStore,
            preselectedWindow: dashboardViewModel.preselectedWindow
        ))
    }

    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: state.account.name) {
                if state.isLoading {
                    ProgressView().controlSize(.small)
                }
            }
            Form {
                Section { header }
                Section("Usage") { gauges }
                Section("History") {
                    AccountDetailView(viewModel: chartViewModel, dashboardViewModel: dashboardViewModel)
                }
                Section("Actions") { actions }
            }
            .formStyle(.grouped)
        }
        // A dashboard card gauge tapped while this pane is already showing.
        .onChange(of: dashboardViewModel.preselectedWindow) { window in
            chartViewModel.selectWindow(window)
        }
        .confirmationDialog("Remove \(state.account.name)?", isPresented: $confirmingRemove, titleVisibility: .visible) {
            Button("Remove Account", role: .destructive) {
                dashboardViewModel.accountStore.removeAccount(id: state.id)
            }
            Button("Cancel", role: .cancel) {}
        }
    }

    private var header: some View {
        HStack(spacing: 14) {
            AccountAvatar(account: state.account, size: 56)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(state.account.name)
                        .font(.title3.weight(.semibold))
                    if dashboardViewModel.isActiveClaudeCodeAccount(state) {
                        Circle()
                            .fill(.green)
                            .frame(width: 8, height: 8)
                            .help("Currently active in Claude Code")
                    }
                }
                if let email = state.account.email, email != state.account.name {
                    Text(email)
                        .foregroundStyle(.secondary)
                }
                statusLine
            }
            Spacer()
            Text(state.account.plan.rawValue)
                .font(.caption.bold())
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(state.account.plan.badgeColor.opacity(0.15))
                .clipShape(Capsule())
        }
        .padding(.vertical, 4)
    }

    /// Same guidance and Re-sync button as `AccountCard.expiredContent`; the
    /// Actions section also keeps a Re-sync row.
    @ViewBuilder private var statusLine: some View {
        if state.account.status == .expired {
            Label("Session expired", systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
                .font(.callout)
            if state.account.source == .manual {
                Text("This key was pasted by hand. Add it again from Settings \u{203A} Accounts, Add Account, \"Paste a session key instead\".")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else if let profileName = state.account.chromeProfileName {
                Text("Open Chrome profile \"\(profileName)\" and login to claude.ai, then re-sync.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Button("Re-sync") {
                Task { await dashboardViewModel.resyncAccount(state.id) }
            }
            .controlSize(.small)
        } else if let error = state.error {
            Text(error)
                .font(.callout)
                .foregroundStyle(.red)
        }
    }

    @ViewBuilder private var gauges: some View {
        if let usage = state.usage {
            UsageGaugeRow(usage: usage, burnRates: state.burnRates, isCompact: false,
                          onOpenChart: { window in chartViewModel.selectWindow(window) })
                .padding(.vertical, 6)
        } else {
            Text(state.isLoading ? "Loading\u{2026}" : "No usage data yet.")
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder private var actions: some View {
        Toggle("Pin to Top", isOn: Binding(
            get: { state.account.isPinned },
            set: { _ in dashboardViewModel.togglePin(for: state.id) }
        ))
        LabeledContent("Run a command for this account") {
            Button("Run Command\u{2026}", action: onRunCommand)
        }
        LabeledContent("Read a fresh session key from the browser") {
            Button("Re-sync") {
                Task { await dashboardViewModel.resyncAccount(state.id) }
            }
        }
        LabeledContent("Remove this account from the dashboard") {
            Button("Remove Account\u{2026}", role: .destructive) {
                confirmingRemove = true
            }
        }
    }
}
