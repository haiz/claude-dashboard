import SwiftUI

struct SettingsView: View {
    @ObservedObject var viewModel: DashboardViewModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack {
            Text("Settings — coming in Task 11")
            Button("Close") { dismiss() }
        }
        .frame(width: 400, height: 300)
        .padding()
    }
}
