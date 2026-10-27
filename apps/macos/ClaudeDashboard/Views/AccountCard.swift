import SwiftUI

struct AccountCard: View {
    let state: AccountUsageState
    let onResync: () -> Void
    let onTogglePin: () -> Void
    var onTap: (() -> Void)? = nil
    var onRefresh: (() -> Void)? = nil
    var onOpenChart: ((UsageWindow) -> Void)? = nil
    var isActiveClaudeCodeAccount: Bool = false
    /// nil hides the Switch button (switcher disabled).
    var switchAvailability: SwitchAvailability? = nil
    var onSwitchClaudeCode: (() -> Void)? = nil
    var isSwitchingClaudeCode: Bool = false
    /// True when this is the home account Claude Code returns to after each 5h reset.
    var isPriorityTarget: Bool = false
    /// nil hides the tick button (account cannot be ticked).
    var onTogglePriority: (() -> Void)? = nil
    /// Non-nil shows the flame: orange for critical, red for high critical.
    var critical: CriticalBadge? = nil
    var isCompact: Bool = true

    /// The header's icon buttons (priority tick, Switch): hover background, tight padding.
    private static let iconButtonStyle = HoverableButtonStyle(horizontalPadding: 3, verticalPadding: 2, cornerRadius: 4)

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                // Header
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text(state.account.name)
                                .font(.title3)
                            if isActiveClaudeCodeAccount {
                                Circle()
                                    .fill(.green)
                                    .frame(width: 8, height: 8)
                                    .help("Currently active in Claude Code")
                            }
                        }
                        if let email = state.account.email, email != state.account.name {
                            Text(email)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                    }

                    if state.account.isPinned {
                        Image(systemName: "pin.fill")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer(minLength: 0)

                    HStack(spacing: 2) {
                        if let critical {
                            Image(systemName: critical.level == .high ? "flame.fill" : "flame")
                                .font(.callout)
                                .foregroundStyle(critical.level == .high ? Color.red : Color.orange)
                                .padding(.horizontal, 3)
                                .help(critical.help)
                                .accessibilityLabel(critical.level.label)
                        }
                        if let onTogglePriority {
                            Button {
                                onTogglePriority()
                            } label: {
                                Image(systemName: isPriorityTarget ? "bolt.fill" : "bolt")
                                    .font(.callout)
                                    .foregroundStyle(isPriorityTarget ? Color.orange : Color.secondary)
                            }
                            .buttonStyle(Self.iconButtonStyle)
                            .help(isPriorityTarget
                                  ? "Ticked: Claude Code comes back here each time its 5h limit resets. Click to cancel"
                                  : "Switch Claude Code here each time its 5h limit resets")
                            .accessibilityLabel("Switch here after 5h reset")
                        }
                        if let switchAvailability, let help = switchAvailability.switchHelp {
                            Button {
                                onSwitchClaudeCode?()
                            } label: {
                                Image(systemName: "arrow.left.arrow.right")
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                            }
                            .buttonStyle(Self.iconButtonStyle)
                            .disabled(isSwitchingClaudeCode)
                            .help(help)
                            .accessibilityLabel("Switch Claude Code to this account")
                        }

                        // Plan badge
                        Text(state.account.plan.rawValue)
                            .font(.caption.bold())
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(state.account.plan.badgeColor.opacity(0.15))
                            .clipShape(Capsule())
                    }

                    if state.isLoading {
                        ProgressView()
                            .controlSize(.small)
                    } else if state.account.status == .expired {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)
                    }
                    // Dots temporarily hidden
                    // else if let usage = state.usage {
                    //     Circle()
                    //         .fill(DashboardViewModel.usageColor(for: usage.fiveHour.utilization))
                    //         .frame(width: 14, height: 14)
                    //     Circle()
                    //         .fill(DashboardViewModel.usageColor(for: usage.sevenDay.utilization))
                    //         .frame(width: 10, height: 10)
                    // }
                }

                if state.account.status == .expired {
                    expiredContent
                } else if let usage = state.usage {
                    usageContent(usage)
                } else if let error = state.error {
                    Text(error)
                        .font(.subheadline)
                        .foregroundStyle(.red)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
        }
        .contextMenu {
            Button {
                onTogglePin()
            } label: {
                Label(
                    state.account.isPinned ? "Unpin" : "Pin to Top",
                    systemImage: state.account.isPinned ? "pin.slash" : "pin"
                )
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            onTap?()
        }
    }

    private func usageContent(_ usage: UsageData) -> some View {
        UsageGaugeRow(usage: usage, burnRates: state.burnRates, isCompact: isCompact, onOpenChart: onOpenChart)
    }

    private var expiredContent: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Session expired.")
                .font(.caption)
                .foregroundStyle(.secondary)

            // A manual record has no profile to open, so the browser line above
            // never renders for it and Re-sync cannot fix it either. Say where
            // the key comes from instead of leaving a dead button.
            if state.account.source == .manual {
                Text("This key was pasted by hand. Add it again from Settings \u{203A} Accounts, Add Account, \"Paste a session key instead\".")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else if let profileName = state.account.chromeProfileName {
                Text("Open Chrome profile \"\(profileName)\" and login to claude.ai, then re-sync.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Button("Re-sync") {
                onResync()
            }
            .controlSize(.small)
            .help("Read a fresh session key from the browser")
        }
    }
}

extension SwitchAvailability {
    /// Tooltip for the Switch control; nil when the control is hidden.
    var switchHelp: String? {
        switch self {
        case .active: return nil
        case .ready: return "Use this account in Claude Code"
        case .notCaptured: return "Sign this account in (opens its browser profile), then switch"
        case .loginExpired: return "Login expired (~30 days); sign in again in the browser, then switch"
        case .needsLogin: return "Claude Code lost this login. Run /login with this account"
        }
    }
}

extension View {
    /// Shows `viewModel.switchMessage` once, then clears it, and — while a Switch is
    /// waiting on a browser sign-in — a cancellable "finish in your browser" prompt.
    /// Also asks to confirm a `viewModel.pendingSwitch`.
    func claudeCodeSwitchAlert(_ viewModel: DashboardViewModel) -> some View {
        alert("Claude Code", isPresented: Binding(
            get: { viewModel.switchMessage != nil },
            set: { if !$0 { viewModel.dismissSwitchMessage() } }
        )) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(viewModel.switchMessage ?? "")
        }
        .alert("Finish signing in", isPresented: Binding(
            get: { viewModel.awaitingBrowserAccount != nil },
            set: { if !$0 { viewModel.cancelClaudeCodeProvisioning() } }
        )) {
            Button("Cancel", role: .cancel) { viewModel.cancelClaudeCodeProvisioning() }
        } message: {
            Text("Claude Dashboard opened your browser profile for this account. Sign in and "
                + "click Authorize to finish, then it switches automatically.")
        }
        // `presenting` hands the button its own copy of the pending switch (see `confirmSwitch`).
        // With Desktop running a checkbox is needed, which an `.alert` cannot hold: a sheet instead.
        .alert("Switch Claude Code?", isPresented: Binding(
            get: { viewModel.pendingSwitch?.desktopRunning == false },
            set: { if !$0 { viewModel.cancelPendingSwitch() } }
        ), presenting: viewModel.pendingSwitch) { pending in
            Button("Switch") { Task { await viewModel.confirmSwitch(pending) } }
            Button("Cancel", role: .cancel) {}
        } message: { pending in
            Text(pending.message)
        }
        .sheet(isPresented: Binding(
            get: { viewModel.pendingSwitch?.desktopRunning == true },
            set: { if !$0 { viewModel.cancelPendingSwitch() } }
        )) {
            DesktopSwitchConfirmSheet(viewModel: viewModel)
        }
        // Shown after the Claude Code message is dismissed, so two prompts never stack.
        .alert("Close Claude Desktop?", isPresented: Binding(
            get: { viewModel.pendingDesktopClose != nil && viewModel.switchMessage == nil
                && viewModel.pendingSwitch == nil },
            set: { if !$0 { viewModel.cancelDesktopClose() } }
        ), presenting: viewModel.pendingDesktopClose) { pending in
            Button("Close and Switch") { Task { await viewModel.confirmDesktopClose(pending) } }
            Button("Not Now", role: .cancel) {}
        } message: { _ in
            Text(DashboardViewModel.desktopCloseMessage)
        }
    }

    /// Popover variant of `claudeCodeSwitchAlert`. In the `MenuBarExtra` panel an `.alert` is a
    /// separate sheet window: clicking it while the app is not frontmost takes key from the
    /// panel, which closes and swallows the click, so the alert returns on reopen. This draws
    /// the same prompts inside the panel instead.
    func claudeCodeSwitchOverlay(_ viewModel: DashboardViewModel) -> some View {
        overlay {
            if let pending = viewModel.pendingSwitch {
                InlinePrompt(title: "Switch Claude Code?", message: pending.message, button: "Switch",
                             buttonHelp: "Switch Claude Code to this account",
                             toggle: pending.desktopRunning
                                ? ("Switch Claude Desktop too", Binding(
                                    get: { viewModel.pendingSwitch?.switchDesktop ?? false },
                                    set: { viewModel.setPendingSwitchDesktop($0) }))
                                : nil,
                             onCancel: { viewModel.cancelPendingSwitch() }) {
                    Task { await viewModel.confirmSwitch(pending) }
                }
            } else if let message = viewModel.switchMessage {
                InlinePrompt(title: "Claude Code", message: message, button: "OK",
                             buttonHelp: "Dismiss this message") {
                    viewModel.dismissSwitchMessage()
                }
            } else if let close = viewModel.pendingDesktopClose {
                InlinePrompt(title: "Close Claude Desktop?", message: DashboardViewModel.desktopCloseMessage,
                             button: "Close and Switch", buttonHelp: "Quit Claude Desktop and switch it",
                             onCancel: { viewModel.cancelDesktopClose() }) {
                    Task { await viewModel.confirmDesktopClose(close) }
                }
            } else if viewModel.awaitingBrowserAccount != nil {
                InlinePrompt(title: "Finish signing in",
                             message: "Claude Dashboard opened your browser profile for this account. Sign in and "
                                + "click Authorize to finish, then it switches automatically.",
                             button: "Cancel",
                             buttonHelp: "Stop waiting for the browser sign-in") {
                    viewModel.cancelClaudeCodeProvisioning()
                }
            }
        }
    }

    /// Scrolls `proxy` to the view tagged `id` whenever `viewModel.scrollToTopRequest` bumps.
    func scrollsToTopAfterSwitch(_ viewModel: DashboardViewModel, proxy: ScrollViewProxy, id: some Hashable) -> some View {
        onChange(of: viewModel.scrollToTopRequest) { _ in
            withAnimation { proxy.scrollTo(id, anchor: .top) }
        }
    }
}

/// The manual Switch confirmation when Claude Desktop is running: the message, the
/// "Switch Claude Desktop too" box (pre-ticked from the setting), Switch and Cancel.
private struct DesktopSwitchConfirmSheet: View {
    @ObservedObject var viewModel: DashboardViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Switch Claude Code?").font(.headline)
            Text(viewModel.pendingSwitch?.message ?? "").fixedSize(horizontal: false, vertical: true)
            Toggle("Switch Claude Desktop too", isOn: Binding(
                get: { viewModel.pendingSwitch?.switchDesktop ?? false },
                set: { viewModel.setPendingSwitchDesktop($0) }))
                .help("Claude Desktop is running. It is closed only after you confirm.")
            HStack {
                Spacer()
                Button("Cancel") { viewModel.cancelPendingSwitch() }
                    .keyboardShortcut(.cancelAction)
                Button("Switch") {
                    // Read before the sheet's dismissal clears `pendingSwitch`.
                    if let pending = viewModel.pendingSwitch {
                        Task { await viewModel.confirmSwitch(pending) }
                    }
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 380)
    }
}
