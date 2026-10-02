import SwiftUI

/// The 5h / 7d / F gauge row, shared by `AccountCard` and `AccountPane`.
/// Moved verbatim from `AccountCard.usageContent`; keep the two-circle gauges,
/// segmented countdown, single-letter labels and animal overlay unchanged.
///
///     UsageGaugeRow(usage: usage, burnRates: state.burnRates, isCompact: false,
///                   onOpenChart: { window in viewModel.openAccount(state.id, window: window) })
struct UsageGaugeRow: View {
    let usage: UsageData
    let burnRates: BurnRates?
    let isCompact: Bool
    let onOpenChart: ((UsageWindow) -> Void)?

    var body: some View {
        HStack(alignment: .top, spacing: isCompact ? 20 : 30) {
            UsageBar(label: "5h", utilization: usage.fiveHour.utilization, resetsAt: usage.fiveHour.resetsAt, totalSeconds: 18000, animal: burnRates?.fiveHour?.animal, isCompact: isCompact, onTap: onOpenChart.map { cb in { cb(.fiveHour) } })
            UsageBar(label: "7d", utilization: usage.sevenDay.utilization, resetsAt: usage.sevenDay.resetsAt, totalSeconds: 604800, animal: burnRates?.sevenDay?.animal, isCompact: isCompact, onTap: onOpenChart.map { cb in { cb(.sevenDay) } })
            // Fable is a weekly window that resets in lockstep with 7d, so hide
            // its countdown to avoid a duplicate clock. Only shown when the
            // account actually has a Fable-scoped limit.
            if let fable = usage.fable {
                UsageBar(label: "F", utilization: fable.utilization, resetsAt: fable.resetsAt, totalSeconds: 604800, animal: burnRates?.fable?.animal, showCountdown: false, isCompact: isCompact, onTap: onOpenChart.map { cb in { cb(.fable) } })
            }
        }
        // Center the gauges so the row stays balanced within the card /
        // popover, which is wider than the gauges themselves.
        .frame(maxWidth: .infinity)
    }
}
