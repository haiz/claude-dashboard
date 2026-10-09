import SwiftUI

/// Main-window pane for one account, laid out like the Apple ID page in System
/// Settings: identity and usage gauges in one box, the usage chart, then actions.
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
                if let availability = dashboardViewModel.switchAvailability[state.id],
                   let help = availability.switchHelp {
                    Button {
                        Task { await dashboardViewModel.requestSwitchClaudeCode(to: state.id) }
                    } label: {
                        Label("Switch", systemImage: "arrow.left.arrow.right")
                    }
                    .disabled(dashboardViewModel.isSwitchingClaudeCode)
                    .help(help)
                }
                // Scoped to this account: a whole-fleet pass would re-fetch usage
                // nobody asked for and clear the error other cards are showing.
                // `refreshAll` skips expired accounts, so Re-sync is the way back.
                Button {
                    Task { await dashboardViewModel.refreshAll(only: [state.id]) }
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(dashboardViewModel.isRefreshing || state.account.status == .expired)
                .help(state.account.status == .expired
                      ? "Session expired. Re-sync to read a fresh key from the browser."
                      : "Fetch the latest usage for this account")
            }
            // A grouped Form caps its content width on macOS, leaving wide
            // empty margins; a ScrollView of PaneSections fills the pane.
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    PaneSection { summary }
                    PaneSection("History") {
                        AccountDetailView(viewModel: chartViewModel, dashboardViewModel: dashboardViewModel)
                    }
                    PaneSection("Actions") { actions }
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 20)
            }
        }
        // The one path that moves the chart: this pane's gauges and picker, and
        // a popover gauge tapped while this account's pane is showing, all write
        // preselectedWindow. The picker keeps it equal to the chart's window, so
        // an unchanged value never means a stale chart.
        .onChange(of: dashboardViewModel.preselectedWindow) { window in
            chartViewModel.selectWindow(window)
        }
        .confirmationDialog("Remove \(state.account.name)?", isPresented: $confirmingRemove, titleVisibility: .visible) {
            Button("Remove Account", role: .destructive) {
                dashboardViewModel.accountStore.removeAccount(id: state.id)
            }
            Button("Cancel", role: .cancel) {}
        }
        .claudeCodeSwitchAlert(dashboardViewModel)
    }

    /// Identity and the usage gauges in one box: avatar, name and status on
    /// the leading side, the gauges at their natural width on the trailing side.
    private var summary: some View {
        HStack(alignment: .center, spacing: 14) {
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
                    Text(state.account.plan.rawValue)
                        .font(.caption.bold())
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(state.account.plan.badgeColor.opacity(0.15))
                        .clipShape(Capsule())
                }
                if let email = state.account.email, email != state.account.name {
                    Text(email)
                        .foregroundStyle(.secondary)
                }
                statusLine
            }
            Spacer(minLength: 20)
            gauges
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
            .help("Read a fresh session key from the browser")
        } else if let error = state.error {
            Text(error)
                .font(.callout)
                .foregroundStyle(.red)
        }
    }

    @ViewBuilder private var gauges: some View {
        if let usage = state.usage {
            UsageGaugeRow(usage: usage, burnRates: state.burnRates, isCompact: false,
                          onOpenChart: { window in dashboardViewModel.openAccount(state.id, window: window) })
                .fixedSize()
        } else {
            Text(state.isLoading ? "Loading\u{2026}" : "No usage data yet.")
                .foregroundStyle(.secondary)
        }
    }

    private var actions: some View {
        VStack(spacing: 10) {
            actionRow("Pin to Top") {
                Toggle("", isOn: Binding(
                    get: { state.account.isPinned },
                    set: { _ in dashboardViewModel.togglePin(for: state.id) }
                ))
                .toggleStyle(.switch)
                .labelsHidden()
                .controlSize(.small)
            }
            Divider()
            actionRow("Run a command for this account") {
                Button("Run Command\u{2026}", action: onRunCommand)
                    .help("Run a saved command for this account")
            }
            Divider()
            actionRow("Read a fresh session key from the browser") {
                Button("Re-sync") {
                    Task { await dashboardViewModel.resyncAccount(state.id) }
                }
                .help("Read a fresh session key from the browser")
            }
            Divider()
            actionRow("Remove this account from the dashboard") {
                Button("Remove Account\u{2026}", role: .destructive) {
                    confirmingRemove = true
                }
                .help("Remove this account from the dashboard")
            }
        }
    }

    private func actionRow<Control: View>(_ title: String, @ViewBuilder control: () -> Control) -> some View {
        HStack {
            Text(title)
            Spacer()
            control()
        }
    }
}

/// A titled, rounded box standing in for a grouped `Form` section, without
/// the Form's width cap.
///
///     PaneSection("Actions") { actions }
///     PaneSection { summary }
private struct PaneSection<Content: View>: View {
    let title: String?
    let content: Content

    init(_ title: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let title {
                Text(title)
                    .font(.headline)
                    .padding(.leading, 4)
            }
            content
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .strokeBorder(Color.primary.opacity(0.08))
                )
        }
    }
}
