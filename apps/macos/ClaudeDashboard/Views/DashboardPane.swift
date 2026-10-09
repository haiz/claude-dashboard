import SwiftUI

/// Dashboard pane: the account card grid (unchanged cards) or the empty state.
///
///     DashboardPane(viewModel: vm, onAddAccount: { showingSetup = true },
///                   onRunCommand: { runCommandAccount = $0 })
struct DashboardPane: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onAddAccount: () -> Void
    let onRunCommand: (Account) -> Void
    /// Same setting as the menu bar popover: off hides the cards' Run Command button.
    @AppStorage(MenuBarPopover.showRunCommandKey, store: AppDefaults.shared)
    private var showRunCommand = false
    private static let scrollTopID = "cards-top"

    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "Dashboard") {
                if !viewModel.accountStates.isEmpty {
                    Button {
                        Task { await viewModel.refreshAll() }
                    } label: {
                        Label("Refresh", systemImage: "arrow.clockwise")
                    }
                    .disabled(viewModel.isRefreshing)
                }
            }
            if viewModel.accountStates.isEmpty {
                emptyStateView
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVGrid(
                            // Cards can show up to three gauges (5h, 7d, and the
                            // Fable model window when present). Size the column to fit
                            // three and cap the max so cards don't stretch and leave a
                            // blank band on the right. Two-gauge cards center their
                            // gauges within the same width.
                            columns: [GridItem(.adaptive(minimum: 420, maximum: 480), spacing: 12, alignment: .top)],
                            alignment: .leading,
                            spacing: 12
                        ) {
                            ForEach(viewModel.accountStates) { state in
                                AccountCard(
                                    state: state,
                                    onResync: { Task { await viewModel.resyncAccount(state.id) } },
                                    onTogglePin: { viewModel.togglePin(for: state.id) },
                                    onRefresh: { Task { await viewModel.refreshAll() } },
                                    onRunCommand: showRunCommand ? { onRunCommand(state.account) } : nil,
                                    onOpenChart: { window in viewModel.openAccount(state.id, window: window) },
                                    isActiveClaudeCodeAccount: viewModel.isActiveClaudeCodeAccount(state),
                                    switchAvailability: viewModel.switchAvailability[state.id],
                                    onSwitchClaudeCode: { Task { await viewModel.requestSwitchClaudeCode(to: state.id) } },
                                    isSwitchingClaudeCode: viewModel.isSwitchingClaudeCode,
                                    isCompact: false
                                )
                            }
                        }
                        .padding(.horizontal, 20)
                        .padding(.bottom, 20)
                        .id(Self.scrollTopID)
                    }
                    .scrollsToTopAfterSwitch(viewModel, proxy: proxy, id: Self.scrollTopID)
                }
            }
        }
        .claudeCodeSwitchAlert(viewModel)
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Spacer()
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No Accounts")
                .font(.title3.bold())
            Text("Sync your Claude accounts from your browser to get started.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Button(action: onAddAccount) {
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
