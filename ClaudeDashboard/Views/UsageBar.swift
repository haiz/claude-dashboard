import SwiftUI

struct UsageBar: View {
    let label: String           // "5h", "7d", or "S"
    let utilization: Double     // 0-100
    let resetsAt: Date?
    let totalSeconds: TimeInterval  // total window: 18000 for 5h, 604800 for 7d
    let animal: String?
    let showCountdown: Bool

    init(label: String, utilization: Double, resetsAt: Date?, totalSeconds: TimeInterval = 18000, animal: String? = nil, showCountdown: Bool = true) {
        self.label = label
        self.utilization = utilization
        self.resetsAt = resetsAt
        self.totalSeconds = totalSeconds
        self.animal = animal
        self.showCountdown = showCountdown
    }

    /// Number of countdown segments: 5 for 5h window, 7 for 7d and Sonnet windows.
    private var segmentCount: Int {
        totalSeconds <= 18000 ? 5 : 7
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label)
                    .font(.system(.body, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(width: 24, alignment: .leading)

                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.primary.opacity(0.1))

                        RoundedRectangle(cornerRadius: 4)
                            .fill(DashboardViewModel.usageColor(for: utilization))
                            .frame(width: geo.size.width * min(utilization / 100, 1.0))

                        if let animal {
                            Text(animal)
                                .font(.system(size: 12))
                                .offset(
                                    x: geo.size.width * min(utilization / 100, 1.0) - 8,
                                    y: -14
                                )
                        }
                    }
                }
                .frame(height: 8)

                Text("\(Int(utilization))%")
                    .font(.system(.body, design: .monospaced))
                    .frame(width: 70, alignment: .trailing)
            }

            if showCountdown, let resetsAt {
                HStack(spacing: 8) {
                    // Spacer matching label width
                    Color.clear
                        .frame(width: 24)

                    CountdownBarsView(
                        resetsAt: resetsAt,
                        totalSeconds: totalSeconds,
                        segmentCount: segmentCount
                    )

                    Text(formatResetTime(resetsAt))
                        .font(.system(.body, design: .monospaced))
                        .foregroundStyle(resetUrgencyColor(resetsAt))
                        .frame(width: 70, alignment: .trailing)
                }
            }
        }
        .padding(.top, animal != nil ? 14 : 0)
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

    /// Formats the reset time as an absolute timestamp, e.g. "Sat 5pm" or "Mon 2am".
    private func formatResetTime(_ date: Date) -> String {
        guard date.timeIntervalSinceNow > 0 else { return "now" }

        let formatter = DateFormatter()
        formatter.locale = Locale.current
        formatter.timeZone = TimeZone.current
        // "EEE" = abbreviated weekday, "ha" = hour + am/pm (e.g. "5PM")
        formatter.dateFormat = "EEE ha"
        let raw = formatter.string(from: date).lowercased()
        return raw.prefix(1).uppercased() + raw.dropFirst()
    }
}

private struct CountdownBarsView: View {
    let resetsAt: Date
    let totalSeconds: TimeInterval
    let segmentCount: Int

    private let segmentColor = Color(red: 74/255, green: 144/255, blue: 217/255) // #4a90d9
    private let depletedColor = Color.primary.opacity(0.08)

    var body: some View {
        GeometryReader { geo in
            HStack(spacing: 2) {
                ForEach(0..<segmentCount, id: \.self) { index in
                    segmentView(index: index)
                }
            }
            .frame(width: geo.size.width * 2.0 / 3.0, height: 4)
            .frame(maxWidth: .infinity, alignment: .trailing)
        }
        .frame(height: 4)
    }

    private func segmentView(index: Int) -> some View {
        let fillFraction = fillFraction(for: index)
        return GeometryReader { geo in
            ZStack(alignment: .leading) {
                // Full background (depleted color)
                RoundedRectangle(cornerRadius: 2)
                    .fill(depletedColor)

                // Blue fill from the right
                if fillFraction > 0 {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(segmentColor)
                        .frame(width: geo.size.width * fillFraction)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                }
            }
        }
    }

    /// Returns 0.0 (fully depleted) to 1.0 (fully remaining) for a segment.
    private func fillFraction(for index: Int) -> Double {
        let remaining = resetsAt.timeIntervalSinceNow
        guard remaining > 0, totalSeconds > 0 else { return 0 }

        let elapsed = max(0, totalSeconds - remaining)
        let secondsPerSegment = totalSeconds / Double(segmentCount)
        let segmentStart = Double(index) * secondsPerSegment
        let segmentEnd = Double(index + 1) * secondsPerSegment

        if elapsed >= segmentEnd {
            return 0.0 // fully depleted
        } else if elapsed <= segmentStart {
            return 1.0 // fully remaining
        } else {
            return (segmentEnd - elapsed) / secondsPerSegment
        }
    }
}
