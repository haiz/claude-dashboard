import SwiftUI

struct DashboardWindow: View {
    @ObservedObject var viewModel: DashboardViewModel
    @State private var showingSettings = false

    var body: some View {
        VStack(spacing: 0) {
            // Toolbar
            HStack {
                Text("Claude Dashboard")
                    .font(.title2.bold())

                Spacer()

                Button(action: {
                    Task { await viewModel.refreshAll() }
                }) {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(viewModel.isRefreshing)

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
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 280, maximum: 400), spacing: 12)],
                        spacing: 12
                    ) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state, onResync: {
                                Task { await viewModel.resyncAccount(state.id) }
                            }, onTogglePin: {
                                viewModel.togglePin(for: state.id)
                            })
                        }
                    }
                    .padding()
                }
            }
        }
        .frame(minWidth: 400, minHeight: 300)
        .sheet(isPresented: $showingSettings) {
            SettingsView(viewModel: viewModel)
        }
    }

    // macOS 13 compatible empty state (ContentUnavailableView requires macOS 14+)
    private var emptyStateView: some View {
        VStack(spacing: 12) {
            Spacer()
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No Accounts")
                .font(.title3.bold())
            Text("Open Settings to sync accounts from Chrome.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Spacer()
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
