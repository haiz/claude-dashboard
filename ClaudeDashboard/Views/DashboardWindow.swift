import SwiftUI

struct DashboardWindow: View {
    @ObservedObject var viewModel: DashboardViewModel
    @State private var showingSettings = false
    @State private var showingSetup = false

    var body: some View {
        Group {
            switch viewModel.navigation {
            case .dashboard:
                dashboardContent
            case .accountDetail(let accountId):
                if let state = viewModel.accountStates.first(where: { $0.id == accountId }) {
                    AccountDetailView(
                        viewModel: AccountDetailViewModel(
                            accountId: accountId,
                            accountName: state.account.name,
                            accountPlan: state.account.plan,
                            logStore: viewModel.logStore
                        ),
                        onBack: { viewModel.navigation = .dashboard }
                    )
                }
            case .overview:
                OverviewChartView(
                    viewModel: viewModel,
                    onBack: { viewModel.navigation = .dashboard }
                )
            }
        }
        .frame(minWidth: 600, minHeight: 450)
        .sheet(isPresented: $showingSettings) {
            SettingsView(viewModel: viewModel)
        }
        .sheet(isPresented: $showingSetup) {
            SetupView(viewModel: viewModel) {
                showingSetup = false
            }
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

                    Button(action: {
                        Task { await viewModel.refreshAll() }
                    }) {
                        Label("Refresh", systemImage: "arrow.clockwise")
                    }
                    .disabled(viewModel.isRefreshing)
                }

                Button(action: { showingSettings = true }) {
                    Label("Settings", systemImage: "gearshape")
                }
            }
            .padding()

            Divider()

            // Cards grid
            if viewModel.accountStates.isEmpty {
                emptyStateView
            } else {
                ScrollView {
                    LazyVStack(spacing: 12) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state, onResync: {
                                Task { await viewModel.resyncAccount(state.id) }
                            }, onTogglePin: {
                                viewModel.togglePin(for: state.id)
                            }, onTap: {
                                viewModel.navigation = .accountDetail(state.id)
                            })
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
            Button(action: { showingSetup = true }) {
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
