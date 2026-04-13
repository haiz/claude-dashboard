import SwiftUI
import AppKit

@main
struct ClaudeDashboardApp: App {
    @StateObject private var viewModel = DashboardViewModel()
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    var body: some Scene {
        MenuBarExtra {
            MenuBarPopover(viewModel: viewModel) {
                appDelegate.openDashboardWindow(viewModel: viewModel)
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "chart.bar.fill")
                Text(viewModel.menuBarLabel)
                    .monospacedDigit()
            }
        }
        .menuBarExtraStyle(.window)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var dashboardWindow: NSWindow?
    private weak var currentViewModel: DashboardViewModel?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return false
    }

    // Intercept the red-X close: hide the window instead of closing it.
    // Keeping the NSWindow instance alive prevents SwiftUI/AppKit from treating
    // this as "last window closed" and terminating the menu-bar-only app.
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard sender === dashboardWindow else { return true }
        // Reset navigation so chart/detail subviews are released.
        currentViewModel?.navigation = .dashboard
        // Drop the SwiftUI view hierarchy to free memory while hidden.
        sender.contentView = nil
        sender.orderOut(nil)
        return false
    }

    func openDashboardWindow(viewModel: DashboardViewModel) {
        currentViewModel = viewModel
        let showSetup = viewModel.accountStore.accounts.isEmpty
        let contentView = DashboardWindowWrapper(viewModel: viewModel, showSetupOnAppear: showSetup)

        let window: NSWindow
        if let existing = dashboardWindow {
            window = existing
        } else {
            window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 1050, height: 750),
                styleMask: [.titled, .closable, .miniaturizable, .resizable],
                backing: .buffered,
                defer: false
            )
            window.title = "Claude Dashboard"
            window.center()
            window.setFrameAutosaveName("ClaudeDashboardWindow")
            window.minSize = NSSize(width: 600, height: 450)
            window.isReleasedWhenClosed = false
            window.delegate = self
            dashboardWindow = window
        }

        window.contentView = NSHostingView(rootView: contentView)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}

/// Wrapper that handles first-time setup sheet via SwiftUI
struct DashboardWindowWrapper: View {
    @ObservedObject var viewModel: DashboardViewModel
    let showSetupOnAppear: Bool
    @State private var showingSetup = false

    var body: some View {
        DashboardWindow(viewModel: viewModel)
            .onAppear {
                if showSetupOnAppear {
                    showingSetup = true
                }
            }
            .sheet(isPresented: $showingSetup) {
                SetupView(viewModel: viewModel) {
                    showingSetup = false
                }
            }
    }
}
