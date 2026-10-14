import SwiftUI

/// The popover's bottom row: Quit on the left, the auto switch toggle on the right and its
/// last event between them. The toggle shows only while `showsAutoSwitchToggle` (on, or near
/// a limit with the widget toggle allowed), dimmed while the active account is not near a limit.
///
///     PopoverFooter(viewModel: viewModel, onQuit: { confirmingQuit = true })
struct PopoverFooter: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onQuit: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Button(action: onQuit) {
                Label("Quit", systemImage: "power")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(HoverableButtonStyle())
            .help("Quit the app and stop monitoring usage")

            Spacer(minLength: 4)

            if viewModel.showsAutoSwitchToggle {
                if let event = viewModel.autoSwitch.lastEvent {
                    (Text(event.text) + Text(" \u{00B7} ") + Text(event.date, style: .time))
                        .font(.caption)
                        .foregroundStyle(event.isFailure ? Color.red : Color.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(event.text)
                }
                Toggle("Auto switch", isOn: Binding(
                    get: { viewModel.autoSwitchSettings.isEnabled },
                    set: { viewModel.setAutoSwitchEnabled($0) }))
                    .toggleStyle(AccentSwitchToggleStyle())
                    .font(.caption)
                    .fixedSize()
                    .opacity(viewModel.isAutoSwitchNearLimit ? 1 : 0.4)
                    .help("Move Claude Code to another account when this one reaches its limit")
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }
}
