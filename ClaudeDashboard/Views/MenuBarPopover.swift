import SwiftUI

private struct HeaderIconButton: View {
    let systemName: String
    let action: () -> Void
    @State private var isHovered = false

    var body: some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.caption)
                .frame(width: 24, height: 24)
                .background(Color.primary.opacity(isHovered ? 0.15 : 0.07))
                .clipShape(RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.borderless)
        .onHover { isHovered = $0 }
    }
}

private struct ScrollItemOffsetKey: PreferenceKey {
    static var defaultValue: [UUID: CGFloat] = [:]
    static func reduce(value: inout [UUID: CGFloat], nextValue: () -> [UUID: CGFloat]) {
        value.merge(nextValue()) { _, new in new }
    }
}

struct MenuBarPopover: View {
    @ObservedObject var viewModel: DashboardViewModel
    @EnvironmentObject var updateViewModel: UpdateViewModel
    let onOpenWindow: () -> Void
    let onOpenOverview: () -> Void
    let onOpenSettings: () -> Void
    let onOpenAccountDetail: (UUID, UsageWindow) -> Void

    @State private var scrollAnchorId: UUID? = nil
    @State private var runCommandAccount: Account? = nil

    var body: some View {
        VStack(spacing: 0) {
            // Update banner shown while downloading/installing
            if case .downloading = updateViewModel.state {
                updateBanner
            } else if case .installing = updateViewModel.state {
                updateBanner
            }

            // Header
            HStack {
                Text("Claude Dashboard")
                    .font(.headline)

                Spacer()

                if !viewModel.accountStates.isEmpty {
                    HeaderIconButton(systemName: "arrow.clockwise") {
                        Task { await viewModel.refreshAll() }
                    }
                    .disabled(viewModel.isRefreshing)
                }

                HeaderIconButton(systemName: "rectangle.expand.vertical") {
                    let popover = NSApp.keyWindow
                    onOpenWindow()
                    popover?.close()
                }

                HeaderIconButton(systemName: "chart.xyaxis.line") {
                    let popover = NSApp.keyWindow
                    onOpenOverview()
                    popover?.close()
                }

                HeaderIconButton(systemName: "gearshape") {
                    let popover = NSApp.keyWindow
                    onOpenSettings()
                    popover?.close()
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)

            Divider()

            // Account cards
            if viewModel.accountStates.isEmpty {
                emptyState
            } else {
                ScrollView {
                    VStack(spacing: 8) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(
                                state: state,
                                onResync: { Task { await viewModel.resyncAccount(state.id) } },
                                onTogglePin: { viewModel.togglePin(for: state.id) },
                                onRefresh: { Task { await viewModel.refreshAll() } },
                                onRunCommand: { runCommandAccount = state.account },
                                onOpenChart: { window in
                                    let popover = NSApp.keyWindow
                                    onOpenAccountDetail(state.id, window)
                                    popover?.close()
                                },
                                isActiveClaudeCodeAccount: viewModel.isActiveClaudeCodeAccount(state),
                                isCompact: true
                            )
                        }
                    }
                    .padding(12)
                }
                .frame(maxHeight: 400)
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
                }
                .buttonStyle(HoverableButtonStyle())

                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
        }
        .frame(width: 320)
        .fixedSize(horizontal: false, vertical: true)
        .overlay {
            if let account = runCommandAccount {
                ZStack {
                    Color.black.opacity(0.4)
                        .ignoresSafeArea()
                        .onTapGesture { runCommandAccount = nil }

                    RunCommandSheet(
                        account: account,
                        isPresented: Binding(
                            get: { runCommandAccount != nil },
                            set: { if !$0 { runCommandAccount = nil } }
                        ),
                        onRefresh: { Task { await viewModel.refreshAll() } }
                    )
                    .background(.regularMaterial)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .padding(12)
                    .frame(width: 320)
                }
            }
        }
    }

    private var updateBanner: some View {
        HStack(spacing: 6) {
            ProgressView()
                .controlSize(.small)
            Text(updateViewModel.state.statusLabel)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 6)
        .background(.yellow.opacity(0.15))
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.largeTitle)
                .foregroundStyle(.secondary)

            Text("No accounts configured")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Button(action: {
                let popover = NSApp.keyWindow
                onOpenWindow()
                popover?.close()
            }) {
                Text("Add Account")
                    .font(.subheadline.weight(.medium))
            }
            .buttonStyle(.borderedProminent)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }
}
