import SwiftUI

struct AccountCard: View {
    let state: AccountUsageState
    let onResync: () -> Void
    let onTogglePin: () -> Void
    var onTap: (() -> Void)? = nil
    var onRefresh: (() -> Void)? = nil
    var onRunCommand: (() -> Void)? = nil
    var onOpenChart: ((UsageWindow) -> Void)? = nil
    var isActiveClaudeCodeAccount: Bool = false
    /// nil hides the Switch button (switcher disabled).
    var switchAvailability: SwitchAvailability? = nil
    var onSwitchClaudeCode: (() -> Void)? = nil
    var isSwitchingClaudeCode: Bool = false
    var isCompact: Bool = true

    @State private var isTerminalHovered = false

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
                        if let switchAvailability, let help = switchAvailability.switchHelp {
                            Button {
                                onSwitchClaudeCode?()
                            } label: {
                                Image(systemName: "arrow.left.arrow.right")
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                                    .padding(2)
                            }
                            .buttonStyle(.plain)
                            .disabled(isSwitchingClaudeCode)
                            .help(help)
                            .accessibilityLabel("Switch Claude Code to this account")
                        }

                        Button {
                            onRunCommand?()
                        } label: {
                            Image(systemName: "terminal")
                                .font(.callout)
                                .foregroundStyle(isTerminalHovered ? .primary : .secondary)
                                .padding(.leading, 4)
                                .padding(.trailing, 2)
                                .padding(.vertical, 2)
                                .background(isTerminalHovered ? Color.primary.opacity(0.1) : Color.clear)
                                .clipShape(RoundedRectangle(cornerRadius: 4))
                        }
                        .buttonStyle(.plain)
                        .onHover { isTerminalHovered = $0 }
                        .help("Run command")

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
    func claudeCodeSwitchAlert(_ viewModel: DashboardViewModel) -> some View {
        alert("Claude Code", isPresented: Binding(
            get: { viewModel.switchMessage != nil },
            set: { if !$0 { viewModel.switchMessage = nil } }
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
    }
}
