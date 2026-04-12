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

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var dashboardWindow: NSWindow?
    private var windowCloseObserver: Any?
    private weak var currentViewModel: DashboardViewModel?

    func openDashboardWindow(viewModel: DashboardViewModel) {
        if let window = dashboardWindow, window.isVisible {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }

        currentViewModel = viewModel
        let showSetup = viewModel.accountStore.accounts.isEmpty
        let contentView = DashboardWindowWrapper(viewModel: viewModel, showSetupOnAppear: showSetup)

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1050, height: 750),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Claude Dashboard"
        window.contentView = NSHostingView(rootView: contentView)
        window.center()
        window.setFrameAutosaveName("ClaudeDashboardWindow")
        window.minSize = NSSize(width: 600, height: 450)

        // Show Dock icon when dashboard window opens
        NSApp.setActivationPolicy(.regular)

        windowCloseObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification,
            object: window,
            queue: .main
        ) { [weak self] _ in
            // Hide Dock icon when dashboard window closes
            NSApp.setActivationPolicy(.accessory)
            // Reset navigation so chart views are released
            self?.currentViewModel?.navigation = .dashboard
            // Release window and its entire view hierarchy to free memory
            self?.dashboardWindow?.contentView = nil
            self?.dashboardWindow = nil
            if let observer = self?.windowCloseObserver {
                NotificationCenter.default.removeObserver(observer)
                self?.windowCloseObserver = nil
            }
        }

        dashboardWindow = window
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
