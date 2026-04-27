import SwiftUI

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

    @State private var scrollAnchorId: UUID? = nil

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
                }

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

                Button(action: {
                    let popover = NSApp.keyWindow
                    onOpenOverview()
                    popover?.close()
                }) {
                    Image(systemName: "chart.xyaxis.line")
                        .font(.caption)
                        .frame(width: 24, height: 24)
                        .background(.quaternary)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                }
                .buttonStyle(.borderless)

                Button(action: {
                    let popover = NSApp.keyWindow
                    onOpenSettings()
                    popover?.close()
                }) {
                    Image(systemName: "gearshape")
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
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 8) {
                            ForEach(viewModel.accountStates) { state in
                                HStack(spacing: 0) {
                                    AccountCard(
                                        state: state,
                                        onResync: { Task { await viewModel.resyncAccount(state.id) } },
                                        onTogglePin: { viewModel.togglePin(for: state.id) },
                                        isActiveClaudeCodeAccount: viewModel.isActiveClaudeCodeAccount(state),
                                        isCompact: true
                                    )
                                    .fixedSize(horizontal: true, vertical: false)
                                    Spacer(minLength: 0)
                                }
                                .id(state.id)
                                .background(
                                    GeometryReader { geo in
                                        Color.clear.preference(
                                            key: ScrollItemOffsetKey.self,
                                            value: [state.id: geo.frame(in: .named("popoverScroll")).minY]
                                        )
                                    }
                                )
                            }
                        }
                        .padding(12)
                    }
                    .coordinateSpace(name: "popoverScroll")
                    .onPreferenceChange(ScrollItemOffsetKey.self) { positions in
                        let topItem = positions.filter { $0.value >= -10 }.min { $0.value < $1.value }
                        if let topItem {
                            scrollAnchorId = topItem.key
                        }
                    }
                    .onChange(of: viewModel.accountStates.map { $0.id }) { _ in
                        guard let id = scrollAnchorId else { return }
                        DispatchQueue.main.async { proxy.scrollTo(id, anchor: .top) }
                    }
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
