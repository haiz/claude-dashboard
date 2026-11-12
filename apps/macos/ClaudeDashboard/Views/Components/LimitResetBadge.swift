import SwiftUI

/// The small "↻1" capsule (arrow symbol + count) beside a gauge label (7d for the Full reset, F for
/// Fable resets). Blue normally, red when the reset expires within 7 days.
/// Its layout height stays under the label's line height (the capsule is drawn
/// as a background that may overhang), so a column with a badge keeps the same
/// label row height as one without, and the rings stay aligned.
///
///     LimitResetBadge(summary: summary, tooltip: "Full reset · expires Dec 10, 22:00", isCompact: true)
struct LimitResetBadge: View {
    let summary: ResetSummary
    let tooltip: String
    let isCompact: Bool

    /// The countdown ring's blue (`UsageBar.segmentColor`).
    static let normalColor = Color(red: 74/255, green: 144/255, blue: 217/255)

    private var fontSize: CGFloat { isCompact ? 8 : 10 }

    var body: some View {
        // The SF Symbol stands in for U+21BB: the monospaced font has no clean glyph for it.
        (Text(Image(systemName: "arrow.clockwise")).font(.system(size: fontSize - 1, weight: .bold))
            + Text("\(summary.count)").font(.system(size: fontSize, weight: .semibold, design: .monospaced)))
            .foregroundStyle(.white)
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, isCompact ? 3 : 4)
            .background(
                Capsule()
                    .fill(summary.urgent ? Color.red : Self.normalColor)
                    .padding(.vertical, -1)
            )
            .help(tooltip)
            .accessibilityLabel(tooltip)
    }
}
