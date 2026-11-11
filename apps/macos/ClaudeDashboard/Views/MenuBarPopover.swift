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
    /// Quit sits right under the card list, so a stray click asks first.
    @State private var confirmingQuit = false
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

            if viewModel.menuBarSyncFailing, let state = viewModel.menuBarState {
                syncErrorBanner(state)
            }

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
                                    onOpenChart: { window in
                                        let popover = NSApp.keyWindow
                                        onOpenAccountDetail(state.id, window)
                                        popover?.close()
                                    },
                                    isActiveClaudeCodeAccount: viewModel.isActiveClaudeCodeAccount(state),
                                    switchAvailability: viewModel.switchAvailability[state.id],
                                    onSwitchClaudeCode: { Task { await viewModel.requestSwitchClaudeCode(to: state.id) } },
                                    isSwitchingClaudeCode: viewModel.isSwitchingClaudeCode,
                                    isPriorityTarget: viewModel.isPriorityTarget(state.id),
                                    onTogglePriority: viewModel.showsPriorityControl(state)
                                        ? { viewModel.togglePriority(for: state.id) } : nil,
                                    critical: viewModel.criticalBadge(for: state),
                                    burnAnimal: viewModel.burnAnimalSource(for: state),
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

            PopoverFooter(viewModel: viewModel, onQuit: { confirmingQuit = true })
        }
        .frame(width: Self.width)
        .fixedSize(horizontal: false, vertical: true)
        .claudeCodeSwitchOverlay(viewModel)
        // Inline, not `.alert`: see `claudeCodeSwitchOverlay` for why the panel can't host one.
        .overlay {
            if confirmingQuit {
                InlinePrompt(title: "Quit Claude Dashboard?",
                             message: "Usage monitoring stops until you open the app again.",
                             button: "Quit",
                             buttonHelp: "Quit the app and stop monitoring usage",
                             onCancel: { confirmingQuit = false }) {
                    NSApplication.shared.terminate(nil)
                }
            }
        }
    }

    /// Why the menu bar shows the red sync mark instead of a percentage.
    private func syncErrorBanner(_ state: AccountUsageState) -> some View {
        HStack(spacing: 6) {
            Image(systemName: MenuBarLabelRenderer.syncErrorSymbol)
                .foregroundStyle(.red)
            Text(Self.syncErrorText(name: state.account.name, lastSuccess: state.account.lastSynced))
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(2)
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(.red.opacity(0.12))
    }

    /// "Sync failing for alice · last success 14:02" (a date is added when not today;
    /// no clause at all when the account never synced).
    static func syncErrorText(name: String, lastSuccess: Date?, now: Date = Date()) -> String {
        let head = "Sync failing for \(name)"
        guard let lastSuccess else { return head }
        let when = Calendar.current.isDate(lastSuccess, inSameDayAs: now)
            ? lastSuccess.formatted(date: .omitted, time: .shortened)
            : lastSuccess.formatted(date: .abbreviated, time: .shortened)
        return "\(head) · last success \(when)"
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
