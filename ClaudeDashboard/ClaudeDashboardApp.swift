import SwiftUI

@main
struct ClaudeDashboardApp: App {
    @StateObject private var viewModel = DashboardViewModel()
    @Environment(\.openWindow) private var openWindow
    @State private var showingSetup = false

    var body: some Scene {
        MenuBarExtra {
            MenuBarPopover(viewModel: viewModel) {
                openWindow(id: "dashboard")
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "chart.bar.fill")
                Text(viewModel.menuBarLabel)
                    .monospacedDigit()
            }
        }
        .menuBarExtraStyle(.window)

        Window("Claude Dashboard", id: "dashboard") {
            DashboardWindow(viewModel: viewModel)
                .onAppear {
                    if viewModel.accountStore.accounts.isEmpty {
                        showingSetup = true
                    }
                }
                .sheet(isPresented: $showingSetup) {
                    SetupView(viewModel: viewModel)
                }
        }
        .defaultSize(width: 700, height: 500)
    }
}
