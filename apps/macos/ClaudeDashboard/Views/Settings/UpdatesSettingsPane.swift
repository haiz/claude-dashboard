import SwiftUI

/// Settings > Updates: the daily auto-update toggle and the manual check.
///
///     UpdatesSettingsPane().environmentObject(updateViewModel)
struct UpdatesSettingsPane: View {
    @EnvironmentObject var updateViewModel: UpdateViewModel

    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "Updates")
            Form {
                Section {
                    Toggle("Auto-update daily", isOn: $updateViewModel.autoUpdateEnabled)
                } footer: {
                    if updateViewModel.autoUpdateEnabled {
                        Text("Checks GitHub once a day and installs new releases automatically.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                Section {
                    LabeledContent("Current version", value: "v\(AppVersion.string)")
                    if let latest = updateViewModel.latestVersion {
                        LabeledContent("Latest version", value: "v\(latest)")
                    }
                    if !updateViewModel.state.statusLabel.isEmpty {
                        LabeledContent("Status") {
                            Text(updateViewModel.state.statusLabel)
                                .foregroundStyle(statusColor(updateViewModel.state))
                        }
                    }
                    HStack {
                        Spacer()
                        checkButton
                    }
                }
            }
            .formStyle(.grouped)
        }
    }

    @ViewBuilder private var checkButton: some View {
        #if DEBUG
        Button("Check for Updates") {}
            .disabled(true)
            .help("Updates only run in release builds")
        #else
        Button(updateViewModel.state.isWorking ? "Working\u{2026}" : "Check for Updates") {
            Task { await updateViewModel.checkNow(autoInstall: true) }
        }
        .disabled(updateViewModel.state.isWorking)
        #endif
    }

    private func statusColor(_ state: UpdateViewModel.State) -> Color {
        switch state {
        case .error: return .red
        case .upToDate: return .green
        default: return .secondary
        }
    }
}
