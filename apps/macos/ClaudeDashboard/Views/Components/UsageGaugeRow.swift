import SwiftUI

/// The 5h / 7d / F gauge row, shared by `AccountCard` and `AccountPane`.
/// Keep the two-circle gauges, segmented countdown and single-letter labels unchanged.
/// The burn animal is drawn in the 5h ring only; 7d and Fable never get one.
/// Limit reset badges sit beside the 7d (Full) and F (Fable) labels.
///
///     UsageGaugeRow(usage: usage, burnAnimal: viewModel.burnAnimalSource(for: state), isCompact: false,
///                   onOpenChart: { window in viewModel.openAccount(state.id, window: window) })
struct UsageGaugeRow: View {
    let usage: UsageData
    /// The 5h ring's animal source; no default, so every caller decides explicitly.
    let burnAnimal: BurnAnimalSource?
    let isCompact: Bool
    let onOpenChart: ((UsageWindow) -> Void)?

    var body: some View {
        // Re-evaluated each minute so a reset that expires, or turns urgent, updates
        // between refreshes.
        TimelineView(.periodic(from: .now, by: 60)) { context in
            let resets = LimitResets.summarize(usage.resetGrants, now: context.date)
            HStack(alignment: .top, spacing: isCompact ? 20 : 30) {
                UsageBar(label: "5h", utilization: usage.fiveHour.utilization, resetsAt: usage.fiveHour.resetsAt, totalSeconds: 18000, burnAnimal: burnAnimal, isCompact: isCompact, onTap: onOpenChart.map { cb in { cb(.fiveHour) } })
                UsageBar(label: "7d", utilization: usage.sevenDay.utilization, resetsAt: usage.sevenDay.resetsAt, totalSeconds: 604800, isCompact: isCompact, resetBadge: badge(.full, resets.full, now: context.date), onTap: onOpenChart.map { cb in { cb(.sevenDay) } })
                // Fable is a weekly window that resets in lockstep with 7d, so hide
                // its countdown to avoid a duplicate clock. Only shown when the
                // account actually has a Fable-scoped limit; a Fable reset on an
                // account without one shows only on the account page.
                if let fable = usage.fable {
                    UsageBar(label: "F", utilization: fable.utilization, resetsAt: fable.resetsAt, totalSeconds: 604800, showCountdown: false, isCompact: isCompact, resetBadge: badge(.fable, resets.fable, now: context.date), onTap: onOpenChart.map { cb in { cb(.fable) } })
                }
            }
            // Center the gauges so the row stays balanced within the card /
            // popover, which is wider than the gauges themselves.
            .frame(maxWidth: .infinity)
        }
    }

    private func badge(_ kind: ResetKind, _ summary: ResetSummary?, now: Date) -> (summary: ResetSummary, tooltip: String)? {
        summary.map { s in
            (s, LimitResetsText.tooltip(kind: kind, summary: s, now: now, calendar: .current, locale: .current))
        }
    }
}
