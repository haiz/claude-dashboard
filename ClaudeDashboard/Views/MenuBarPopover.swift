import SwiftUI

struct MenuBarPopover: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onOpenWindow: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("Claude Dashboard")
                    .font(.headline)

                Spacer()

                Button(action: {
                    Task { await viewModel.refreshAll() }
                }) {
                    Image(systemName: "arrow.clockwise")
                        .font(.caption)
                        .frame(width: 24, height: 24)
                        .background(.quaternary)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                }
                .buttonStyle(.borderless)
                .disabled(viewModel.isRefreshing)

                Button(action: {
                    let popover = NSApp.keyWindow
                    onOpenWindow()
                    popover?.close()
                }) {
                    Image(systemName: "rectangle.expand.vertical")
                        .font(.caption)
                        .frame(width: 24, height: 24)
                        .background(.quaternary)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)

            Divider()

            // Account cards
            if viewModel.accountStates.isEmpty {
                emptyState
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state, onResync: {
                                Task { await viewModel.resyncAccount(state.id) }
                            }, onTogglePin: {
                                viewModel.togglePin(for: state.id)
                            })
                        }
                    }
                    .padding(12)
                }
            }

            Divider()

            // Footer with Quit
            HStack {
                Button(action: {
                    NSApplication.shared.terminate(nil)
                }) {
                    Label("Quit Claude Dashboard", systemImage: "power")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(.quaternary)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                }
                .buttonStyle(.borderless)

                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
        }
        .frame(width: 320)
        .frame(maxHeight: 500)
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.largeTitle)
                .foregroundStyle(.secondary)

            Text("No accounts configured")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Text("Open Settings to sync from Chrome")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }
}
