import SwiftUI

/// Tools > Switch Log: every auto switch, failed attempt and no-candidate stretch, newest first.
///
///     AutoSwitchLogView(viewModel: switchLogViewModel)
struct AutoSwitchLogView: View {
    @ObservedObject var viewModel: AutoSwitchLogViewModel
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
        PaneHeader(title: "Switch Log") {
            Button(action: { Task { await viewModel.load() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .help("Reload the switch log")
            Button(role: .destructive, action: { confirmingClear = true }) {
                Label("Clear All", systemImage: "trash")
            }
            .disabled(viewModel.entries.isEmpty)
            .help("Delete every switch log entry")
        }
        .confirmationDialog("Clear the switch log?", isPresented: $confirmingClear, titleVisibility: .visible) {
            Button("Clear All", role: .destructive) { Task { await viewModel.clear() } }
            Button("Cancel", role: .cancel) {}
        }
    }

    private func row(_ entry: AutoSwitchLogEntry) -> some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(Self.dateFormatter.string(from: entry.date))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                    Text(accounts(entry))
                        .font(.caption.weight(.medium))
                }
                Text(entry.reason)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                if let detail = entry.detail {
                    Text(detail)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            Spacer(minLength: 0)
            Text(entry.outcome.label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(color(entry.outcome))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    private func accounts(_ entry: AutoSwitchLogEntry) -> String {
        let from = entry.fromName ?? "Unknown account"
        guard let to = entry.toName else { return from }
        return "\(from) \u{2192} \(to)"
    }

    private func color(_ outcome: AutoSwitchOutcome) -> Color {
        switch outcome {
        case .switched: return .green
        case .failed: return .red
        case .noCandidate: return .orange
        }
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Spacer()
            Image(systemName: "arrow.left.arrow.right")
                .font(.system(size: 40))
                .foregroundStyle(.secondary)
            Text("No auto switches yet.")
                .foregroundStyle(.secondary)
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
