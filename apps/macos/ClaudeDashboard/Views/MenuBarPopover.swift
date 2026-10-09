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
    let onOpenSettings: () -> Void
    let onOpenHelp: () -> Void
    let onOpenAccountDetail: (UUID, UsageWindow) -> Void

    @State private var scrollAnchorId: UUID? = nil
    @State private var runCommandAccount: Account? = nil
    /// Off by default: the card's Run Command button is easy to hit by mistake
    /// and costs header space. Settings > General flips it.
    @AppStorage(MenuBarPopover.showRunCommandKey, store: AppDefaults.shared)
    private var showRunCommand = false
    static let showRunCommandKey = "menuBarShowRunCommand"
    private static let scrollTopID = "cards-top"
    /// Wide enough for a compact card's three gauge groups plus `cardListPadding`
    /// on each side; narrower and the card overflows the scroll view, which
    /// then draws it flush left (`AccountCardLayoutTests` guards this). Sized for
    /// the longest reset labels ("12:49 AM", "Wed 12:50am").
    static let width: CGFloat = 352
    static let cardListPadding: CGFloat = 12

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
                    .help("Refresh usage for all accounts")
                }

                HeaderIconButton(systemName: "rectangle.expand.vertical") {
                    let popover = NSApp.keyWindow
                    onOpenWindow()
                    popover?.close()
                }
                .help("Open the main window")

                HeaderIconButton(systemName: "questionmark.circle") {
                    let popover = NSApp.keyWindow
                    onOpenHelp()
                    popover?.close()
                }
                .help("Help")

                HeaderIconButton(systemName: "gearshape") {
                    let popover = NSApp.keyWindow
                    onOpenSettings()
                    popover?.close()
                }
                .help("Open Settings")
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)

            Divider()

            // Account cards
            if viewModel.accountStates.isEmpty {
                emptyState
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        VStack(spacing: 8) {
                            ForEach(viewModel.accountStates) { state in
                                AccountCard(
                                    state: state,
                                    onResync: { Task { await viewModel.resyncAccount(state.id) } },
                                    onTogglePin: { viewModel.togglePin(for: state.id) },
                                    onRefresh: { Task { await viewModel.refreshAll() } },
                                    onRunCommand: showRunCommand ? { runCommandAccount = state.account } : nil,
                                    onOpenChart: { window in
                                        let popover = NSApp.keyWindow
                                        onOpenAccountDetail(state.id, window)
                                        popover?.close()
                                    },
                                    isActiveClaudeCodeAccount: viewModel.isActiveClaudeCodeAccount(state),
                                    switchAvailability: viewModel.switchAvailability[state.id],
                                    onSwitchClaudeCode: { Task { await viewModel.requestSwitchClaudeCode(to: state.id) } },
                                    isSwitchingClaudeCode: viewModel.isSwitchingClaudeCode,
                                    isCompact: true
                                )
                            }
                        }
                        .padding(Self.cardListPadding)
                        .id(Self.scrollTopID)
                    }
                    // A legacy (always shown) scroller would take its width from
                    // the right side only, leaving the cards off-center.
                    .scrollIndicators(.never)
                    .scrollsToTopAfterSwitch(viewModel, proxy: proxy, id: Self.scrollTopID)
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
                .help("Quit the app and stop monitoring usage")

                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
        }
        .frame(width: Self.width)
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
                        runner: viewModel.commandRunner,
                        onRefresh: { Task { await viewModel.refreshAll() } }
                    )
                    .background(.regularMaterial)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .padding(12)
                    .frame(width: Self.width)
                }
            }
        }
        .claudeCodeSwitchOverlay(viewModel)
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
            .help("Open the main window to add a Claude account")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }
}
