import SwiftUI

/// Settings > Auto Refresh: on/off plus the interval (1-60 minutes).
///
///     RefreshSettingsPane(viewModel: vm)
struct RefreshSettingsPane: View {
    @ObservedObject var viewModel: DashboardViewModel

    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "Auto Refresh")
            Form {
                Section {
                    Toggle("Enable auto refresh", isOn: $viewModel.autoRefreshEnabled)
                    if viewModel.autoRefreshEnabled {
                        LabeledContent("Interval") {
                            Stepper("\(viewModel.autoRefreshMinutes) min", value: $viewModel.autoRefreshMinutes, in: 1...60)
                        }
                    }
                }
            }
            .formStyle(.grouped)
        }
    }
}
