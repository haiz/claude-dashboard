import SwiftUI
import AppKit

/// About: app icon, name and version, centered like the macOS About pages.
///
///     AboutPane()
struct AboutPane: View {
    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "About")
            VStack(spacing: 8) {
                Spacer()
                Image(nsImage: NSApp.applicationIconImage)
                    .resizable()
                    .frame(width: 96, height: 96)
                Text("Claude Dashboard")
                    .font(.title.bold())
                Text("Version \(AppVersion.string)")
                    .foregroundStyle(.secondary)
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}
