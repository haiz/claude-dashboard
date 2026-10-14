import SwiftUI
import AppKit

/// Settings > General: app identity (icon, name, version), auto refresh
/// (on/off plus a 1-60 minute interval), the cards' Run Command button (menu bar
/// and Dashboard), auto switch (on/off, 5h and 7d thresholds, menu bar toggle),
/// and updates (daily toggle plus the manual check).
///
///     GeneralSettingsPane(viewModel: vm).environmentObject(updateViewModel)
struct GeneralSettingsPane: View {
    @ObservedObject var viewModel: DashboardViewModel
    @EnvironmentObject var updateViewModel: UpdateViewModel
    @AppStorage(MenuBarPopover.showRunCommandKey, store: AppDefaults.shared)
    private var showRunCommandOnCards = false

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
                Section("Cards") {
                    Toggle("Show Run Command button on cards", isOn: $showRunCommandOnCards)
                }
                Section {
                    Toggle("Ping when a usage window is not running", isOn: Binding(
                        get: { viewModel.pingController.isAutoPingEnabled },
                        set: { viewModel.setAutoPingEnabled($0) }))
                } header: {
                    Text("Auto Ping")
                } footer: {
                    Text("Starts a new window for every active account whose 5h or 7d window is not running. Each ping sends one short message from that account.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Section {
                    Toggle("Enable auto switch", isOn: Binding(
                        get: { viewModel.autoSwitchSettings.isEnabled },
                        set: { viewModel.setAutoSwitchEnabled($0) }))
                    LabeledContent("Switch at 5h usage") {
                        Stepper("\(viewModel.autoSwitchSettings.fiveHourThreshold)%",
                                value: switchBinding(\.fiveHourThreshold),
                                in: AutoSwitchSettings.fiveHourRange)
                    }
                    LabeledContent("Switch at 7d usage") {
                        Stepper("\(viewModel.autoSwitchSettings.sevenDayThreshold)%",
                                value: switchBinding(\.sevenDayThreshold),
                                in: AutoSwitchSettings.sevenDayRange)
                    }
                    Toggle("Show toggle in menu bar when near limit",
                           isOn: switchBinding(\.showWidgetToggle))
                } header: {
                    Text("Auto Switch")
                } footer: {
                    Text("Moves Claude Code to the account whose weekly quota is closest to expiring unused. Only accounts already signed in to Claude Code are used; no browser opens.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
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

    /// `autoSwitchSettings` is a `let` on the view model, so `$viewModel...` cannot reach it.
    private func switchBinding<Value>(_ keyPath: ReferenceWritableKeyPath<AutoSwitchSettings, Value>) -> Binding<Value> {
        Binding(get: { viewModel.autoSwitchSettings[keyPath: keyPath] },
                set: { viewModel.autoSwitchSettings[keyPath: keyPath] = $0 })
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
        .help("Check GitHub for a newer version and install it")
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
