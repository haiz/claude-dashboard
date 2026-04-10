import SwiftUI

struct UsageBar: View {
    let label: String           // "5h", "7d", or "S"
    let utilization: Double     // 0-100
    let resetsAt: Date?
    let totalSeconds: TimeInterval  // total window: 18000 for 5h, 604800 for 7d

    init(label: String, utilization: Double, resetsAt: Date?, totalSeconds: TimeInterval = 18000) {
        self.label = label
        self.utilization = utilization
        self.resetsAt = resetsAt
        self.totalSeconds = totalSeconds
    }

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
                    .foregroundStyle(resetUrgencyColor(resetsAt))
                    .padding(.leading, 28)
            }
        }
    }

    /// Color based on how close the reset is relative to total window.
    /// Near reset (low ratio) = green. Far from reset (high ratio) = muted/tertiary.
    private func resetUrgencyColor(_ date: Date) -> Color {
        let remaining = date.timeIntervalSinceNow
        guard remaining > 0, totalSeconds > 0 else {
            return .green
        }

        let fraction = min(remaining / totalSeconds, 1.0)

        // fraction > 0.3: show as muted secondary (long time until reset)
        // fraction 0.0-0.3: interpolate to green (near reset = use freely)
        if fraction > 0.3 {
            return .secondary.opacity(0.6)
        }

        let greenIntensity = 1.0 - (fraction / 0.3)
        return Color(hue: 120.0 / 360.0, saturation: 0.6 * greenIntensity + 0.1, brightness: 0.5 + 0.35 * greenIntensity)
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
