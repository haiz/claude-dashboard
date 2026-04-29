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
        HStack(alignment: .top, spacing: isCompact ? 20 : 30) {
            UsageBar(label: "5h", utilization: usage.fiveHour.utilization, resetsAt: usage.fiveHour.resetsAt, totalSeconds: 18000, animal: state.burnRates?.fiveHour?.animal, isCompact: isCompact, onTap: onOpenChart.map { cb in { cb(.fiveHour) } })
            UsageBar(label: "7d", utilization: usage.sevenDay.utilization, resetsAt: usage.sevenDay.resetsAt, totalSeconds: 604800, animal: state.burnRates?.sevenDay?.animal, isCompact: isCompact, onTap: onOpenChart.map { cb in { cb(.sevenDay) } })
            if let sonnet = usage.sevenDaySonnet {
                UsageBar(label: "S", utilization: sonnet.utilization, resetsAt: sonnet.resetsAt, totalSeconds: 604800, animal: state.burnRates?.sonnet?.animal, showCountdown: false, isCompact: isCompact, onTap: onOpenChart.map { cb in { cb(.sonnet) } })
            }
        }
    }

    private var expiredContent: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Session expired.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let profileName = state.account.chromeProfileName {
                Text("Open Chrome profile \"\(profileName)\" and login to claude.ai, then re-sync.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Button("Re-sync from Chrome") {
                onResync()
            }
            .controlSize(.small)
        }
    }
}
