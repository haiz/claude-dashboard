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
        HStack(alignment: .top, spacing: 8) {
            Text(label)
                .font(.system(.body, design: .monospaced))
                .foregroundStyle(.secondary)
                .frame(width: 24, alignment: .leading)
                .padding(.top, 14)

            CircularProgressView(utilization: utilization, animal: animal)

            if showCountdown, let resetsAt {
                VStack(spacing: 4) {
                    CircularCountdownView(
                        resetsAt: resetsAt,
                        totalSeconds: totalSeconds,
                        segmentCount: segmentCount,
                        color: segmentColor(resetsAt)
                    )
                    Text(formatResetTime(resetsAt))
                        .font(.system(size: 9, design: .monospaced))
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private func segmentColor(_ date: Date) -> Color {
        let remaining = date.timeIntervalSinceNow
        guard remaining > 0, totalSeconds > 0 else { return .green }
        let fraction = min(remaining / totalSeconds, 1.0)
        if fraction > 0.3 {
            return Color(red: 74/255, green: 144/255, blue: 217/255)
        }
        let greenIntensity = 1.0 - (fraction / 0.3)
        return Color(hue: 120.0 / 360.0, saturation: 0.6 * greenIntensity + 0.1, brightness: 0.5 + 0.35 * greenIntensity)
    }

    private func formatResetTime(_ date: Date) -> String {
        guard date.timeIntervalSinceNow > 0 else { return "now" }
        let formatter = DateFormatter()
        formatter.locale = Locale.current
        formatter.timeZone = TimeZone.current
        if totalSeconds <= 18000 {
            formatter.timeStyle = .short
            formatter.dateStyle = .none
            return formatter.string(from: date)
        }
        formatter.dateFormat = "EEE ha"
        let raw = formatter.string(from: date).lowercased()
        return raw.prefix(1).uppercased() + raw.dropFirst()
    }
}


private struct CircularProgressView: View {
    let utilization: Double
    let animal: String?

    private var fillFraction: Double { min(utilization / 100.0, 1.0) }

    var body: some View {
        ZStack {
            Circle()
                .stroke(Color.primary.opacity(0.2), lineWidth: 5)
            Circle()
                .trim(from: 0, to: fillFraction)
                .stroke(
                    DashboardViewModel.usageColor(for: utilization),
                    style: StrokeStyle(lineWidth: 5, lineCap: .round)
                )
                .rotationEffect(.degrees(-90))
            Text("\(Int(utilization))%")
                .font(.system(size: 11, design: .monospaced))
                .minimumScaleFactor(0.8)
        }
        .overlay(alignment: .bottomTrailing) {
            if let animal {
                Text(animal)
                    .font(.system(size: 10))
                    .offset(x: 4, y: 4)
            }
        }
        .frame(width: 48, height: 48)
    }
}

private struct CircularCountdownView: View {
    let resetsAt: Date
    let totalSeconds: TimeInterval
    let segmentCount: Int
    let color: Color

    // circumference = π * 48pt ≈ 150.796pt; each gap = 2pt
    private let gapFraction: Double = 2.0 / (Double.pi * 48.0)

    private var segmentFraction: Double {
        (1.0 - Double(segmentCount) * gapFraction) / Double(segmentCount)
    }

    private func segStart(_ i: Int) -> Double {
        Double(i) * (segmentFraction + gapFraction)
    }

    private func segEnd(_ i: Int) -> Double {
        segStart(i) + segmentFraction
    }

    private func fillFraction(for index: Int) -> Double {
        let remaining = resetsAt.timeIntervalSinceNow
        guard remaining > 0, totalSeconds > 0 else { return 0 }
        let elapsed = max(0, totalSeconds - remaining)
        let secondsPerSegment = totalSeconds / Double(segmentCount)
        let timeStart = Double(index) * secondsPerSegment
        let timeEnd = Double(index + 1) * secondsPerSegment
        if elapsed >= timeEnd { return 0.0 }
        else if elapsed <= timeStart { return 1.0 }
        else { return (timeEnd - elapsed) / secondsPerSegment }
    }

    var body: some View {
        ZStack {
            ForEach(0..<segmentCount, id: \.self) { i in
                Circle()
                    .trim(from: segStart(i), to: segEnd(i))
                    .stroke(Color.primary.opacity(0.08), style: StrokeStyle(lineWidth: 5, lineCap: .butt))
            }
            ForEach(0..<segmentCount, id: \.self) { i in
                let fill = fillFraction(for: i)
                if fill > 0 {
                    Circle()
                        .trim(from: segStart(i), to: segStart(i) + segmentFraction * fill)
                        .stroke(color, style: StrokeStyle(lineWidth: 5, lineCap: .butt))
                }
            }
        }
        .rotationEffect(.degrees(-90))
        .frame(width: 48, height: 48)
    }
}
