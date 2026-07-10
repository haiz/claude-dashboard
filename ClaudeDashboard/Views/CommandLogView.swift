// ClaudeDashboard/Views/CommandLogView.swift
import SwiftUI

struct CommandLogView: View {
    @ObservedObject var viewModel: CommandLogViewModel
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
        .frame(minWidth: 640, minHeight: 400)
        .task { await viewModel.load() }
    }

    private var header: some View {
        HStack {
            Text("Command Log")
                .font(.title2.bold())
            Spacer()
            Button(action: { Task { await viewModel.load() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            Button(role: .destructive, action: { confirmingClear = true }) {
                Label("Clear All", systemImage: "trash")
            }
            .disabled(viewModel.entries.isEmpty)
        }
        .padding()
        .confirmationDialog("Clear all command logs?", isPresented: $confirmingClear, titleVisibility: .visible) {
            Button("Clear All", role: .destructive) { Task { await viewModel.clear() } }
            Button("Cancel", role: .cancel) {}
        }
    }

    private func row(_ entry: CommandLogEntry) -> some View {
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
                Text(entry.command)
                    .font(.system(.callout, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            VStack(alignment: .trailing, spacing: 4) {
                statusView(entry)
                Text(durationText(entry))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .help(entry.output?.isEmpty == false ? entry.output! : "No output captured")
    }

    private func triggerBadge(_ trigger: CommandTrigger) -> some View {
        Text(trigger.label)
            .font(.caption2.weight(.semibold))
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(badgeColor(trigger).opacity(0.18))
            .foregroundStyle(badgeColor(trigger))
            .clipShape(Capsule())
    }

    private func badgeColor(_ trigger: CommandTrigger) -> Color {
        switch trigger {
        case .manual: return .blue
        case .autoReset: return .purple
        case .autoEmpty: return .orange
        }
    }

    @ViewBuilder
    private func exitView(_ code: Int32?) -> some View {
        if let code {
            Text("exit \(code)")
                .font(.caption.monospacedDigit().weight(.semibold))
                .foregroundStyle(code == 0 ? Color.green : Color.red)
        } else {
            Text("—")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func statusView(_ entry: CommandLogEntry) -> some View {
        switch entry.status {
        case .exited:
            exitView(entry.exitCode)
        default:
            Text(entry.status.label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(statusColor(entry.status))
        }
    }

    private func statusColor(_ status: CommandStatus) -> Color {
        switch status {
        case .exited: return .secondary
        case .timedOut: return .orange
        case .cancelled: return .secondary
        case .launchedInTerminal: return .blue
        case .launchFailed: return .red
        }
    }

    private func durationText(_ entry: CommandLogEntry) -> String {
        guard let finished = entry.finishedAt else { return "—" }
        let secs = finished.timeIntervalSince(entry.startedAt)
        return String(format: "%.1fs", max(0, secs))
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Spacer()
            Image(systemName: "terminal")
                .font(.system(size: 40))
                .foregroundStyle(.secondary)
            Text("No commands have run yet.")
                .foregroundStyle(.secondary)
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
