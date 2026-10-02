import SwiftUI

/// The main window's sidebar, grouped like System Settings. Selecting a row
/// goes through `DashboardViewModel.selectFromSidebar`; `MainWindow` renders the matching pane.
///
///     NavigationSplitView { SidebarView(viewModel: vm) } detail: { ... }
struct SidebarView: View {
    @ObservedObject var viewModel: DashboardViewModel

    var body: some View {
        List(selection: selectionBinding) {
            Section("Usage") {
                ForEach(SidebarItem.usageItems, id: \.self) { staticRow($0) }
            }
            if !viewModel.accountStates.isEmpty {
                Section("Accounts") {
                    ForEach(viewModel.accountStates) { state in
                        accountRow(state).tag(SidebarItem.account(state.id))
                    }
                }
            }
            Section("Tools") {
                ForEach(SidebarItem.toolItems, id: \.self) { staticRow($0) }
            }
            Section("Settings") {
                ForEach(SidebarItem.settingsItems, id: \.self) { staticRow($0) }
            }
        }
        .listStyle(.sidebar)
        .navigationSplitViewColumnWidth(min: 200, ideal: 230, max: 300)
    }

    /// `List` selection is optional and a click on empty space clears it; keep the
    /// current pane instead of rendering nothing.
    private var selectionBinding: Binding<SidebarItem?> {
        Binding(
            get: { viewModel.selection },
            set: { if let item = $0 { viewModel.selectFromSidebar(item) } }
        )
    }

    private func staticRow(_ item: SidebarItem) -> some View {
        Label {
            Text(item.style?.title ?? "")
        } icon: {
            if let style = item.style {
                SettingsIcon(systemImage: style.systemImage, color: style.tileColor)
            }
        }
        .tag(item)
    }

    private func accountRow(_ state: AccountUsageState) -> some View {
        HStack(spacing: 8) {
            AccountAvatar(account: state.account, size: 20)
            Text(state.account.name)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 4)
            if state.account.status == .expired {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
            } else if let peak = state.peakUtilization {
                Text("\(Int(peak))%")
                    .monospacedDigit()
                    .foregroundStyle(DashboardViewModel.usageColor(for: peak))
            }
        }
    }
}
