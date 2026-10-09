import SwiftUI
import AppKit

/// Settings > General: app identity (icon, name, version), auto refresh
/// (on/off plus a 1-60 minute interval), the menu bar's Run Command button,
/// and updates (daily toggle plus the manual check).
///
///     GeneralSettingsPane(viewModel: vm).environmentObject(updateViewModel)
struct GeneralSettingsPane: View {
    @ObservedObject var viewModel: DashboardViewModel
    @EnvironmentObject var updateViewModel: UpdateViewModel
    @AppStorage(MenuBarPopover.showRunCommandKey, store: AppDefaults.shared)
    private var showRunCommandInMenuBar = false

    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "General")
            Form {
                Section { about }
                Section("Auto Refresh") {
                    Toggle("Enable auto refresh", isOn: $viewModel.autoRefreshEnabled)
                    if viewModel.autoRefreshEnabled {
                        LabeledContent("Interval") {
                            Stepper("\(viewModel.autoRefreshMinutes) min", value: $viewModel.autoRefreshMinutes, in: 1...60)
                        }
                    }
                }
                Section("Menu Bar") {
                    Toggle("Show Run Command button on cards", isOn: $showRunCommandInMenuBar)
                }
                Section {
                    Toggle("Auto-update daily", isOn: $updateViewModel.autoUpdateEnabled)
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
                } header: {
                    Text("Updates")
                } footer: {
                    if updateViewModel.autoUpdateEnabled {
                        Text("Checks GitHub once a day and installs new releases automatically.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)
        }
    }

    private var about: some View {
        VStack(spacing: 6) {
            Image(nsImage: NSApp.applicationIconImage)
                .resizable()
                .frame(width: 64, height: 64)
            Text("Claude Dashboard")
                .font(.title2.bold())
            Text("Version \(AppVersion.string)")
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
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
