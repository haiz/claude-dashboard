import SwiftUI

/// The popover's bottom row: Quit on the left, the auto switch toggle on the right. The toggle
/// shows only while `showsAutoSwitchToggle` (on, or near a limit with the widget toggle
/// allowed), dimmed while the active account is not near a limit. The app version sits at the
/// exact center: both sides take equal halves of the rest. Switch events live in the Switch Log.
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
            .frame(maxWidth: .infinity, alignment: .leading)

            Text("v\(AppVersion.string)")
                .font(.caption)
                .foregroundStyle(.tertiary)
                .lineLimit(1)
                .truncationMode(.tail)
                .help("Claude Dashboard \(AppVersion.string)")

            HStack(spacing: 8) {
                if viewModel.showsAutoSwitchToggle {
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
            .frame(maxWidth: .infinity, alignment: .trailing)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }
}
