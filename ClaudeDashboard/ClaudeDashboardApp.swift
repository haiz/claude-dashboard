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
            .onAppear {
                let key = "claude-dashboard.hasLaunchedBefore"
                if !UserDefaults.standard.bool(forKey: key) {
                    UserDefaults.standard.set(true, forKey: key)
                    appDelegate.openDashboardWindow(viewModel: viewModel)
                }
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
        // Dismiss any presented sheets (Settings, Setup) so AppKit
        // removes the dimming overlay before we hide the window.
        while let sheet = sender.attachedSheet {
            sender.endSheet(sheet)
        }
        // Reset navigation so chart/detail subviews are released.
        currentViewModel?.navigation = .dashboard
        // Drop the SwiftUI view hierarchy to free memory while hidden.
        sender.contentView = nil
        sender.orderOut(nil)
        NSApp.setActivationPolicy(.accessory)  // hide dock icon
        return false
    }

    // Re-raise the dashboard when the user clicks the dock icon or re-launches the app
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if let window = dashboardWindow, window.isVisible {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
        } else if let viewModel = currentViewModel {
            openDashboardWindow(viewModel: viewModel)
        }
        return false
    }

    func openDashboardWindow(viewModel: DashboardViewModel) {
        currentViewModel = viewModel
        let showSetup = viewModel.accountStore.accounts.isEmpty
        let contentView = DashboardWindowWrapper(viewModel: viewModel, showSetupOnAppear: showSetup)

        let window: NSWindow
        if let existing = dashboardWindow {
            window = existing
            // Dismiss any lingering sheets from a previous session.
            while let sheet = window.attachedSheet {
                window.endSheet(sheet)
            }
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
        NSApp.setActivationPolicy(.regular)  // show dock icon
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
