import SwiftUI

/// Settings > General > Startup: the "Open at login" switch. Re-reads the login-item list on
/// appear, so a change made in System Settings > General > Login Items shows here. When macOS
/// wants approval, or a change fails, the footer says so.
///
///     LaunchAtLoginSection(controller: viewModel.launchAtLogin,
///                          openHelp: { viewModel.helpFocus = $0; viewModel.selection = .help })
struct LaunchAtLoginSection: View {
    @ObservedObject var controller: LaunchAtLoginController
    let openHelp: (HelpTopic) -> Void

    var body: some View {
        Section {
            Toggle(isOn: Binding(get: { controller.isOn }, set: { controller.setEnabled($0) })) {
                HStack(spacing: 4) {
                    Text("Open at login")
                    InfoTip(SettingsTips.launchAtLogin) { openHelp(SettingsTips.launchAtLogin.topic) }
                }
            }
        } header: {
            Text("Startup")
        } footer: {
            if controller.needsApproval {
                HStack {
                    Text("Allow Claude Dashboard in System Settings > General > Login Items.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Open Login Items") { controller.openLoginItems() }
                        .controlSize(.small)
                }
            } else if let message = controller.errorMessage {
                Text(message)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
        .onAppear { controller.refresh() }
    }
}
