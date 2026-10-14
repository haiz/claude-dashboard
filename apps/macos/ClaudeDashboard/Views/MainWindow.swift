import SwiftUI

/// The single main window: a System Settings-style sidebar and the selected pane.
/// The Setup sheet hangs off this view so any pane can open it.
///
///     MainWindow(viewModel: vm, showSetupOnAppear: vm.accountStore.accounts.isEmpty)
///         .environmentObject(updateViewModel)
struct MainWindow: View {
    @ObservedObject var viewModel: DashboardViewModel
    let showSetupOnAppear: Bool

    @StateObject private var pingLogViewModel: PingLogViewModel
    @State private var showingSetup = false
    @State private var columnVisibility: NavigationSplitViewVisibility = .all

    init(viewModel: DashboardViewModel, showSetupOnAppear: Bool) {
        self.viewModel = viewModel
        self.showSetupOnAppear = showSetupOnAppear
        _pingLogViewModel = StateObject(wrappedValue: PingLogViewModel(
            store: viewModel.pingController.logStore,
            accountStore: viewModel.accountStore,
            onClear: { viewModel.pingController.clearLatest() }
        ))
    }

    var body: some View {
        NavigationSplitView(columnVisibility: $columnVisibility) {
            SidebarView(viewModel: viewModel)
        } detail: {
            pane
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                // Lift the pane so its PaneHeader sits on the titlebar row, the way System
                // Settings does. Not `.ignoresSafeArea`: header buttons laid out that way got
                // no mouse clicks on that row, while a lifted pane's buttons do. With the
                // sidebar collapsed the traffic lights and sidebar toggle own that row.
                .padding(.top, columnVisibility == .detailOnly ? 0 : -36)
                // Card edges line up with the header buttons only without a scroller.
                .scrollIndicators(.never)
        }
        .frame(minWidth: 900, minHeight: 560)
        // Sheets get it too: each one is set below rather than trusting inheritance.
        .buttonStyle(HoverBorderedButtonStyle())
        .onAppear {
            if showSetupOnAppear { showingSetup = true }
        }
        .sheet(isPresented: $showingSetup) {
            SetupView(viewModel: viewModel) {
                showingSetup = false
            }
            .buttonStyle(HoverBorderedButtonStyle())
        }
    }

    @ViewBuilder private var pane: some View {
        switch viewModel.selection {
        case .dashboard:
            DashboardPane(viewModel: viewModel, onAddAccount: { showingSetup = true })
        case .overview:
            OverviewChartView(viewModel: viewModel)
        case .account(let id):
            if let state = viewModel.accountStates.first(where: { $0.id == id }) {
                AccountPane(dashboardViewModel: viewModel, state: state)
                    .id(id)
            }
        case .pingLog:
            PingLogView(viewModel: pingLogViewModel)
        case .help:
            HelpView()
        case .settingsAccounts:
            AccountsSettingsPane(viewModel: viewModel, onAddAccount: { showingSetup = true })
        case .settingsGeneral:
            GeneralSettingsPane(viewModel: viewModel)
        }
    }
}
