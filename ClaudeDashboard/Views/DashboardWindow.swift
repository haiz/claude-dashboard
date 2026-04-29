import SwiftUI

struct DashboardWindow: View {
    @ObservedObject var viewModel: DashboardViewModel
    var onAddAccount: (() -> Void)?
    @State private var runCommandAccount: Account? = nil

    var body: some View {
        Group {
            switch viewModel.navigation {
            case .dashboard:
                dashboardContent
            case .accountDetail(let accountId, let preselectedWindow):
                if let state = viewModel.accountStates.first(where: { $0.id == accountId }) {
                    AccountDetailView(
                        viewModel: AccountDetailViewModel(
                            accountId: accountId,
                            accountName: state.account.name,
                            accountPlan: state.account.plan,
                            logStore: viewModel.logStore,
                            preselectedWindow: preselectedWindow
                        ),
                        dashboardViewModel: viewModel,
                        onBack: { viewModel.navigation = .dashboard },
                        onAllAccounts: { viewModel.navigation = .overview }
                    )
                }
            case .overview:
                OverviewChartView(
                    viewModel: viewModel,
                    onBack: { viewModel.navigation = .dashboard }
                )
            }
        }
        .frame(minWidth: 1050, minHeight: 450)
        .sheet(isPresented: $viewModel.isPresentingSettings) {
            SettingsView(viewModel: viewModel)
        }
        .sheet(item: $runCommandAccount) { account in
            RunCommandSheet(
                account: account,
                isPresented: Binding(
                    get: { runCommandAccount?.id == account.id },
                    set: { if !$0 { runCommandAccount = nil } }
                ),
                onRefresh: { Task { await viewModel.refreshAll() } }
            )
        }
    }

    private var dashboardContent: some View {
        VStack(spacing: 0) {
            // Toolbar
            HStack {
                Text("Claude Dashboard")
                    .font(.title2.bold())

                Spacer()

                if !viewModel.accountStates.isEmpty {
                    Button(action: { viewModel.navigation = .overview }) {
                        Label("Overview", systemImage: "chart.xyaxis.line")
                    }
                    .buttonStyle(HoverableButtonStyle(prominent: true))

                    Button(action: {
                        Task { await viewModel.refreshAll() }
                    }) {
                        Label("Refresh", systemImage: "arrow.clockwise")
                    }
                    .buttonStyle(HoverableButtonStyle(prominent: true))
                    .disabled(viewModel.isRefreshing)
                }

                Button(action: { viewModel.isPresentingSettings = true }) {
                    Label("Settings", systemImage: "gearshape")
                }
                .buttonStyle(HoverableButtonStyle(prominent: true))
            }
            .padding()

            Divider()

            // Cards grid
            if viewModel.accountStates.isEmpty {
                emptyStateView
            } else {
                ScrollView {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 440), spacing: 12, alignment: .top)],
                        alignment: .leading,
                        spacing: 12
                    ) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(
                                state: state,
                                onResync: { Task { await viewModel.resyncAccount(state.id) } },
                                onTogglePin: { viewModel.togglePin(for: state.id) },
                                onRefresh: { Task { await viewModel.refreshAll() } },
                                onRunCommand: { runCommandAccount = state.account },
                                onOpenChart: { window in viewModel.navigation = .accountDetail(state.id, window) },
                                isActiveClaudeCodeAccount: viewModel.isActiveClaudeCodeAccount(state),
                                isCompact: false
                            )
                        }
                    }
                    .padding()
                }
            }
        }
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Spacer()
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No Accounts")
                .font(.title3.bold())
            Text("Sync your Claude accounts from Chrome to get started.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Button(action: { onAddAccount?() }) {
                Text("Add Account")
                    .font(.body.weight(.medium))
            }
            .buttonStyle(.borderedProminent)
            Spacer()
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
