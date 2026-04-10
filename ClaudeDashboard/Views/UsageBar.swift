import SwiftUI

struct UsageBar: View {
    let label: String           // "5h" or "7d"
    let utilization: Double     // 0-100
    let resetsAt: Date?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(width: 24, alignment: .leading)

                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.primary.opacity(0.1))

                        RoundedRectangle(cornerRadius: 4)
                            .fill(DashboardViewModel.usageColor(for: utilization))
                            .frame(width: geo.size.width * min(utilization / 100, 1.0))
                    }
                }
                .frame(height: 8)

                Text("\(Int(utilization))%")
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 36, alignment: .trailing)
            }

            if let resetsAt {
                Text("resets in \(formatTimeRemaining(resetsAt))")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .padding(.leading, 28)
            }
        }
    }

    private func formatTimeRemaining(_ date: Date) -> String {
        let remaining = date.timeIntervalSinceNow
        guard remaining > 0 else { return "now" }

        let days = Int(remaining) / 86400
        let hours = (Int(remaining) % 86400) / 3600
        let minutes = (Int(remaining) % 3600) / 60

        if days > 0 {
            return "\(days)d \(hours)h"
        } else if hours > 0 {
            return "\(hours)h \(String(format: "%02d", minutes))m"
        } else {
            return "\(minutes)m"
        }
    }
}
