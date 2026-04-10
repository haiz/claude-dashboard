import SwiftUI

struct AccountCard: View {
    let state: AccountUsageState
    let onResync: () -> Void

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                // Header
                HStack {
                    Text(state.account.name)
                        .font(.headline)

                    Spacer()

                    if state.isLoading {
                        ProgressView()
                            .controlSize(.small)
                    } else if state.account.status == .expired {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)
                    } else if let usage = state.usage {
                        Circle()
                            .fill(DashboardViewModel.usageColor(for: usage.fiveHour.utilization))
                            .frame(width: 10, height: 10)
                    }
                }

                if state.account.status == .expired {
                    expiredContent
                } else if let usage = state.usage {
                    usageContent(usage)
                } else if let error = state.error {
                    Text(error)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            .padding(.vertical, 4)
        }
    }

    private func usageContent(_ usage: UsageData) -> some View {
        VStack(spacing: 8) {
            UsageBar(label: "5h", utilization: usage.fiveHour.utilization, resetsAt: usage.fiveHour.resetsAt)
            UsageBar(label: "7d", utilization: usage.sevenDay.utilization, resetsAt: usage.sevenDay.resetsAt)
        }
    }

    private var expiredContent: some View {
        VStack(spacing: 8) {
            Text("Session expired.")
                .font(.caption)
                .foregroundStyle(.secondary)

            Button("Re-sync from Chrome") {
                onResync()
            }
            .controlSize(.small)
        }
    }
}
