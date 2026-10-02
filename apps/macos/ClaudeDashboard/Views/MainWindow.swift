import SwiftUI

/// The single main window: a System Settings-style sidebar and the selected pane.
/// Sheets (Setup, Run Command) hang off this view so any pane can open them.
///
///     MainWindow(viewModel: vm, showSetupOnAppear: vm.accountStore.accounts.isEmpty)
///         .environmentObject(updateViewModel)
struct MainWindow: View {
    @ObservedObject var viewModel: DashboardViewModel
    let showSetupOnAppear: Bool

    @StateObject private var commandLogViewModel: CommandLogViewModel
    @State private var showingSetup = false
    @State private var runCommandAccount: Account?

    init(viewModel: DashboardViewModel, showSetupOnAppear: Bool) {
        self.viewModel = viewModel
        self.showSetupOnAppear = showSetupOnAppear
        _commandLogViewModel = StateObject(wrappedValue: CommandLogViewModel(
            store: viewModel.commandLogStore,
            accountStore: viewModel.accountStore
        ))
    }

    var body: some View {
        NavigationSplitView {
            SidebarView(viewModel: viewModel)
        } detail: {
            pane
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .frame(minWidth: 900, minHeight: 560)
        .onAppear {
            if showSetupOnAppear { showingSetup = true }
        }
        .sheet(isPresented: $showingSetup) {
            SetupView(viewModel: viewModel) {
                showingSetup = false
            }
        }
        .sheet(item: $runCommandAccount) { account in
            RunCommandSheet(
                account: account,
                isPresented: Binding(
                    get: { runCommandAccount?.id == account.id },
                    set: { if !$0 { runCommandAccount = nil } }
                ),
                runner: viewModel.commandRunner,
                onRefresh: { Task { await viewModel.refreshAll() } }
            )
        }
    }

    @ViewBuilder private var pane: some View {
        switch viewModel.selection {
        case .dashboard:
            DashboardPane(viewModel: viewModel,
                          onAddAccount: { showingSetup = true },
                          onRunCommand: { runCommandAccount = $0 })
        case .overview:
            OverviewChartView(viewModel: viewModel)
        case .account(let id):
            if let state = viewModel.accountStates.first(where: { $0.id == id }) {
                AccountPane(dashboardViewModel: viewModel, state: state,
                            onRunCommand: { runCommandAccount = state.account })
                    .id(id)
            }
        case .commandLog:
            CommandLogView(viewModel: commandLogViewModel)
        case .help:
            HelpView()
        case .settingsAccounts:
            AccountsSettingsPane(viewModel: viewModel, onAddAccount: { showingSetup = true })
        case .settingsGeneral:
            GeneralSettingsPane(viewModel: viewModel)
        }
    }
}
