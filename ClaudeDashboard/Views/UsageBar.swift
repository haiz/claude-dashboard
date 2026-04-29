import SwiftUI

struct UsageBar: View {
    let label: String           // "5h", "7d", or "S"
    let utilization: Double     // 0-100
    let resetsAt: Date?
    let totalSeconds: TimeInterval
    let animal: String?
    let showCountdown: Bool
    let isCompact: Bool         // true = widget/popover, false = dashboard

    var onTap: (() -> Void)? = nil

    init(label: String, utilization: Double, resetsAt: Date?, totalSeconds: TimeInterval = 18000, animal: String? = nil, showCountdown: Bool = true, isCompact: Bool = true, onTap: (() -> Void)? = nil) {
        self.label = label
        self.utilization = utilization
        self.resetsAt = resetsAt
        self.totalSeconds = totalSeconds
        self.animal = animal
        self.showCountdown = showCountdown
        self.isCompact = isCompact
        self.onTap = onTap
    }

    private var largeDiameter: CGFloat { isCompact ? 52 : 68 }
    private var smallDiameter: CGFloat { isCompact ? 34 : 44 }
    private var largeLineWidth: CGFloat { isCompact ? 6 : 8 }
    private var smallLineWidth: CGFloat { isCompact ? 4 : 5 }
    private var labelFontSize: CGFloat { isCompact ? 10 : 13 }

    /// Number of countdown segments: 5 for 5h window, 7 for 7d and Sonnet windows.
    private var segmentCount: Int { totalSeconds <= 18000 ? 5 : 7 }

    var body: some View {
        VStack(alignment: .center, spacing: 5) {
            Text(label)
                .font(.system(size: labelFontSize, design: .monospaced))
                .foregroundStyle(.secondary)

            HStack(alignment: .bottom, spacing: isCompact ? 5 : 8) {
                CircularProgressView(
                    utilization: utilization,
                    animal: animal,
                    diameter: largeDiameter,
                    lineWidth: largeLineWidth,
                    onTap: onTap
                )

                if showCountdown, let resetsAt {
                    CountdownColumn(
                        resetsAt: resetsAt,
                        totalSeconds: totalSeconds,
                        segmentCount: segmentCount,
                        color: segmentColor(resetsAt),
                        diameter: smallDiameter,
                        lineWidth: smallLineWidth,
                        onTap: onTap
                    )
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
}

private struct CountdownColumn: View {
    let resetsAt: Date
    let totalSeconds: TimeInterval
    let segmentCount: Int
    let color: Color
    let diameter: CGFloat
    let lineWidth: CGFloat
    var onTap: (() -> Void)? = nil

    @State private var isHovered = false

    var body: some View {
        VStack(spacing: 2) {
            Text(formatResetTime(resetsAt))
                .font(.system(size: 8, design: .monospaced))
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .fixedSize()

            Group {
                if let action = onTap {
                    Button(action: action) { clockContent }
                        .buttonStyle(.plain)
                } else {
                    clockContent
                }
            }
            .scaleEffect(isHovered ? 1.06 : 1.0)
            .shadow(color: color.opacity(isHovered ? 0.35 : 0), radius: isHovered ? 6 : 0)
            .animation(.spring(response: 0.28, dampingFraction: 0.75), value: isHovered)
            .onHover { hovering in
                isHovered = hovering
                if onTap != nil {
                    if hovering { NSCursor.pointingHand.push() } else { NSCursor.pop() }
                }
            }
        }
    }

    private var clockContent: some View {
        TimelineView(.periodic(from: .now, by: 1)) { _ in
            ZStack {
                CircularCountdownView(
                    resetsAt: resetsAt,
                    totalSeconds: totalSeconds,
                    segmentCount: segmentCount,
                    color: color,
                    diameter: diameter,
                    lineWidth: lineWidth
                )
                Text(formattedCountdown(resetsAt))
                    .font(.system(size: diameter * 0.24, design: .monospaced))
                    .minimumScaleFactor(0.6)
                    .lineLimit(1)
                    .foregroundStyle(.primary.opacity(0.85))
            }
            .frame(width: diameter, height: diameter)
        }
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

    private func formattedCountdown(_ date: Date) -> String {
        let remaining = date.timeIntervalSinceNow
        guard remaining > 0 else { return "0:00" }
        let totalSecs = Int(remaining)
        if totalSeconds <= 18000 {
            let hours = totalSecs / 3600
            let minutes = (totalSecs % 3600) / 60
            let seconds = totalSecs % 60
            if hours > 0 {
                return "\(hours):\(String(format: "%02d", minutes))"
            }
            return "\(minutes):\(String(format: "%02d", seconds))"
        } else {
            let days = totalSecs / 86400
            let hours = (totalSecs % 86400) / 3600
            let minutes = (totalSecs % 3600) / 60
            if days > 0 {
                return "\(days)d\(hours)h"
            }
            return "\(hours):\(String(format: "%02d", minutes))"
        }
    }
}

private struct CircularProgressView: View {
    let utilization: Double
    let animal: String?
    let diameter: CGFloat
    let lineWidth: CGFloat
    var onTap: (() -> Void)? = nil

    @State private var isHovered = false

    private var fillFraction: Double { min(utilization / 100.0, 1.0) }
    private var ringColor: Color { DashboardViewModel.usageColor(for: utilization) }

    var body: some View {
        Group {
            if let action = onTap {
                Button(action: action) { circleContent }
                    .buttonStyle(.plain)
            } else {
                circleContent
            }
        }
        .scaleEffect(isHovered ? 1.06 : 1.0)
        .shadow(color: ringColor.opacity(isHovered ? 0.35 : 0), radius: isHovered ? 6 : 0)
        .animation(.spring(response: 0.28, dampingFraction: 0.75), value: isHovered)
        .onHover { hovering in
            isHovered = hovering
            if onTap != nil {
                if hovering { NSCursor.pointingHand.push() } else { NSCursor.pop() }
            }
        }
    }

    private var circleContent: some View {
        ZStack {
            Circle()
                .stroke(Color.primary.opacity(0.2), lineWidth: lineWidth)
            Circle()
                .trim(from: 0, to: fillFraction)
                .stroke(
                    ringColor.opacity(isHovered ? 1.0 : 0.92),
                    style: StrokeStyle(lineWidth: lineWidth, lineCap: .round)
                )
                .rotationEffect(.degrees(-90))

            HStack(alignment: .firstTextBaseline, spacing: 0) {
                Text("\(Int(utilization))")
                    .font(.system(size: diameter * 0.33, weight: .semibold, design: .rounded))
                    .minimumScaleFactor(0.7)
                Text("%")
                    .font(.system(size: diameter * 0.20, weight: .regular))
            }
            .lineLimit(1)
        }
        .overlay(alignment: .bottomTrailing) {
            if let animal {
                Text(animal)
                    .font(.system(size: 10))
                    .offset(x: 4, y: 4)
            }
        }
        .frame(width: diameter, height: diameter)
    }
}

private struct CircularCountdownView: View {
    let resetsAt: Date
    let totalSeconds: TimeInterval
    let segmentCount: Int
    let color: Color
    let diameter: CGFloat
    let lineWidth: CGFloat

    private var gapFraction: Double { 2.0 / (Double.pi * diameter) }

    private var segmentFraction: Double {
        (1.0 - Double(segmentCount) * gapFraction) / Double(segmentCount)
    }

    private func segStart(_ i: Int) -> Double {
        Double(i) * (segmentFraction + gapFraction)
    }

    private func segEnd(_ i: Int) -> Double {
        segStart(i) + segmentFraction
    }

    var body: some View {
        let remaining = max(0, resetsAt.timeIntervalSinceNow)
        let fillFrac = totalSeconds > 0 ? min(1.0, remaining / totalSeconds) : 0.0
        // Arc ends at segEnd(last), i.e. 1 - gapFraction; start moves forward as time passes.
        let arcRange = 1.0 - gapFraction

        ZStack {
            ForEach(0..<segmentCount, id: \.self) { i in
                Circle()
                    .trim(from: segStart(i), to: segEnd(i))
                    .stroke(Color.primary.opacity(0.08), style: StrokeStyle(lineWidth: lineWidth, lineCap: .butt))
            }
            if fillFrac > 0 {
                let fillStart = (1.0 - fillFrac) * arcRange
                ForEach(0..<segmentCount, id: \.self) { i in
                    let segS = segStart(i)
                    let segE = segEnd(i)
                    if segE > fillStart {
                        Circle()
                            .trim(from: max(segS, fillStart), to: segE)
                            .stroke(color, style: StrokeStyle(lineWidth: lineWidth, lineCap: .butt))
                    }
                }
            }
        }
        .rotationEffect(.degrees(-90))
        .frame(width: diameter, height: diameter)
    }
}
