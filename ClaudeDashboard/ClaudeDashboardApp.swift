import SwiftUI

@main
struct ClaudeDashboardApp: App {
    var body: some Scene {
        MenuBarExtra("Claude Dashboard", systemImage: "chart.bar.fill") {
            Text("Claude Dashboard")
                .padding()
        }
        .menuBarExtraStyle(.window)
    }
}
