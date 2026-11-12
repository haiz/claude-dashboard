import SwiftUI

/// The account page's "Limit resets" box: one read-only line per kind present,
/// e.g. "Full reset · 1 — Expires Dec 10, 22:00 (in 28 days)", with the expiry
/// in red when it is under 7 days away. Hidden when no reset is live. The app
/// only shows resets; they are used on claude.ai, which "Open Usage Page" opens.
///
///     LimitResetsSection(grants: usage.resetGrants) { UsagePage.open(account) }
struct LimitResetsSection: View {
    let grants: [ResetGrant]
    let openUsagePage: () -> Void

    var body: some View {
        // Re-evaluated each minute so the relative text and the red rule stay current.
        TimelineView(.periodic(from: .now, by: 60)) { context in
            let resets = LimitResets.summarize(grants, now: context.date)
            if resets.full != nil || resets.fable != nil {
                PaneSection("Limit resets") {
                    HStack(alignment: .center, spacing: 12) {
                        VStack(alignment: .leading, spacing: 6) {
                            if let full = resets.full { line(.full, full, now: context.date) }
                            if let fable = resets.fable { line(.fable, fable, now: context.date) }
                        }
                        Spacer(minLength: 0)
                        Button(action: openUsagePage) {
                            Label("Open Usage Page", systemImage: "arrow.up.forward.square")
                        }
                        .help("Open claude.ai's usage page in the browser this account signs in with")
                    }
                }
            }
        }
    }

    private func line(_ kind: ResetKind, _ summary: ResetSummary, now: Date) -> some View {
        let text = LimitResetsText.detailLine(kind: kind, summary: summary, now: now,
                                              calendar: .current, locale: .current)
        return HStack(spacing: 6) {
            Text(text.title)
            Text("\u{2014}")
                .foregroundStyle(.secondary)
            Text(text.expiry)
                .foregroundStyle(summary.urgent ? Color.red : Color.secondary)
        }
    }
}
