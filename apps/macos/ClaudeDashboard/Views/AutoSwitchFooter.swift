import SwiftUI

/// The popover footer's auto switch row: the toggle and the last event. Renders nothing
/// unless `showsAutoSwitchToggle` (on, or near a limit with the widget toggle allowed).
///
///     AutoSwitchFooter(viewModel: viewModel)
struct AutoSwitchFooter: View {
    @ObservedObject var viewModel: DashboardViewModel

    var body: some View {
        if viewModel.showsAutoSwitchToggle {
            HStack(spacing: 8) {
                Toggle("Auto switch", isOn: Binding(
                    get: { viewModel.autoSwitchSettings.isEnabled },
                    set: { viewModel.setAutoSwitchEnabled($0) }))
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .font(.caption)
                    .help("Move Claude Code to another account when this one reaches its limit")
                Spacer(minLength: 4)
                if let event = viewModel.autoSwitch.lastEvent {
                    (Text(event.text) + Text(" \u{00B7} ") + Text(event.date, style: .time))
                        .font(.caption)
                        .foregroundStyle(event.isFailure ? Color.red : Color.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(event.text)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
        }
    }
}
