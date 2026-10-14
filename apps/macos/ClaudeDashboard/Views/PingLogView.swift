import SwiftUI

/// Tools > Ping Log: every ping, newest first.
///
///     PingLogView(viewModel: pingLogViewModel)
struct PingLogView: View {
    @ObservedObject var viewModel: PingLogViewModel
    @State private var confirmingClear = false

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .medium
        return f
    }()

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if viewModel.entries.isEmpty {
                emptyState
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(viewModel.entries) { entry in
                            row(entry)
                            Divider()
                        }
                    }
                }
            }
        }
        .task { await viewModel.load() }
    }

    private var header: some View {
        PaneHeader(title: "Ping Log") {
            Button(action: { Task { await viewModel.load() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .help("Reload the ping log")
            Button(role: .destructive, action: { confirmingClear = true }) {
                Label("Clear All", systemImage: "trash")
            }
            .disabled(viewModel.entries.isEmpty)
            .help("Delete every ping log entry")
        }
        .confirmationDialog("Clear the ping log?", isPresented: $confirmingClear, titleVisibility: .visible) {
            Button("Clear All", role: .destructive) { Task { await viewModel.clear() } }
            Button("Cancel", role: .cancel) {}
        }
    }

    private func row(_ entry: PingLogEntry) -> some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(Self.dateFormatter.string(from: entry.startedAt))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                    triggerBadge(entry.trigger)
                    Text(viewModel.accountName(for: entry.accountId))
                        .font(.caption.weight(.medium))
                }
                if let detail = entry.detail {
                    Text(detail)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            Spacer(minLength: 0)
            VStack(alignment: .trailing, spacing: 4) {
                Text(entry.outcome.label)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(entry.outcome == .ok ? Color.green : Color.red)
                Text(String(format: "%.1fs", max(0, entry.finishedAt.timeIntervalSince(entry.startedAt))))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    private func triggerBadge(_ trigger: PingTrigger) -> some View {
        let color: Color = trigger == .manual ? .blue : .purple
        return Text(trigger.label)
            .font(.caption2.weight(.semibold))
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.18))
            .foregroundStyle(color)
            .clipShape(Capsule())
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Spacer()
            Image(systemName: "dot.radiowaves.left.and.right")
                .font(.system(size: 40))
                .foregroundStyle(.secondary)
            Text("No pings yet.")
                .foregroundStyle(.secondary)
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
