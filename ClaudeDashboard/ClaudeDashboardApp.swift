import SwiftUI
import AppKit
import Combine

private enum MenuBarLabelRenderer {
    private static let barH: CGFloat = 22

    static func render(percent: String, time: String?) -> NSImage {
        let pctFont = NSFont.monospacedDigitSystemFont(ofSize: 14, weight: .bold)
        let timeFont = NSFont.monospacedDigitSystemFont(ofSize: 8, weight: .bold)
        let col = NSColor.black

        let pctStr = NSAttributedString(string: percent,
                                        attributes: [.font: pctFont, .foregroundColor: col])
        let pctSz = pctStr.size()

        var textW = ceil(pctSz.width)
        var timeSz = CGSize.zero
        var timeStr: NSAttributedString?
        if let t = time {
            let ts = NSAttributedString(string: t,
                                        attributes: [.font: timeFont, .foregroundColor: col])
            timeSz = ts.size()
            timeStr = ts
            textW = max(textW, ceil(timeSz.width))
        }

        let iconCfg = NSImage.SymbolConfiguration(pointSize: 20, weight: .regular)
        let iconImg = (NSImage(systemSymbolName: "chart.bar.fill", accessibilityDescription: nil)?
            .withSymbolConfiguration(iconCfg))!
        let iconSz = iconImg.size

        // Squeeze the 3-bar icon horizontally to 2/3 of its natural width, keep height.
        let iconDrawW = ceil(iconSz.width * 2.0 / 3.0)
        let iconGap: CGFloat = 3
        let totalW = iconDrawW + iconGap + textW
        let sz = NSSize(width: totalW, height: barH)

        let lineGap: CGFloat = 0.5

        let image = NSImage(size: sz, flipped: false) { _ in
            iconImg.draw(in: NSRect(x: 0, y: (barH - iconSz.height) / 2,
                                    width: iconDrawW, height: iconSz.height))
            if let ts = timeStr {
                // Bottom-align time so its descender tip sits at image bottom (y=0),
                // then stack pct directly above time's cap top.
                let timeCapTop = abs(timeFont.descender) + timeFont.capHeight
                let timeOriginY: CGFloat = 0
                let pctOriginY = timeCapTop + lineGap
                pctStr.draw(at: NSPoint(x: totalW - pctSz.width, y: pctOriginY))
                ts.draw(at: NSPoint(x: totalW - timeSz.width, y: timeOriginY))
            } else {
                let pctVisualH = abs(pctFont.descender) + pctFont.capHeight
                let originY = (barH - pctVisualH) / 2
                pctStr.draw(at: NSPoint(x: iconDrawW + iconGap, y: originY))
            }
            return true
        }
        image.isTemplate = true
        return image
    }
}

@main
struct ClaudeDashboardApp: App {
    @StateObject private var viewModel = DashboardViewModel()
    @StateObject private var updateViewModel = UpdateViewModel()
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    var body: some Scene {
        MenuBarExtra {
            MenuBarPopover(
                viewModel: viewModel,
                onOpenWindow: {
                    appDelegate.openDashboardWindow(viewModel: viewModel, updateViewModel: updateViewModel)
                },
                onOpenOverview: {
                    viewModel.navigation = .overview
                    appDelegate.openDashboardWindow(viewModel: viewModel, updateViewModel: updateViewModel)
                },
                onOpenSettings: {
                    viewModel.isPresentingSettings = true
                    appDelegate.openDashboardWindow(viewModel: viewModel, updateViewModel: updateViewModel)
                },
                onOpenAccountDetail: { accountId, window in
                    viewModel.navigation = .accountDetail(accountId, window)
                    appDelegate.openDashboardWindow(viewModel: viewModel, updateViewModel: updateViewModel)
                }
            )
            .environmentObject(updateViewModel)
            .onAppear {
                appDelegate.updateViewModel = updateViewModel
                updateViewModel.startBackgroundChecks()
                let key = "claude-dashboard.hasLaunchedBefore"
                if !UserDefaults.standard.bool(forKey: key) {
                    UserDefaults.standard.set(true, forKey: key)
                    appDelegate.openDashboardWindow(viewModel: viewModel, updateViewModel: updateViewModel)
                }
            }
        } label: {
            Image(nsImage: MenuBarLabelRenderer.render(
                percent: viewModel.menuBarPercentText,
                time: viewModel.menuBarTimeText
            ))
        }
        .menuBarExtraStyle(.window)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var dashboardWindow: NSWindow?
    private weak var currentViewModel: DashboardViewModel?
    weak var updateViewModel: UpdateViewModel?
    private var navigationCancellable: AnyCancellable?

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
            openDashboardWindow(viewModel: viewModel, updateViewModel: updateViewModel)
        }
        return false
    }

    @MainActor func openDashboardWindow(viewModel: DashboardViewModel, updateViewModel: UpdateViewModel? = nil) {
        currentViewModel = viewModel
        if let uvm = updateViewModel { self.updateViewModel = uvm }
        let showSetup = viewModel.accountStore.accounts.isEmpty
        let uvm = self.updateViewModel ?? UpdateViewModel()
        let contentView = DashboardWindowWrapper(viewModel: viewModel, showSetupOnAppear: showSetup)
            .environmentObject(uvm)

        let window: NSWindow
        if let existing = dashboardWindow {
            window = existing
            // Dismiss any lingering sheets from a previous session.
            while let sheet = window.attachedSheet {
                window.endSheet(sheet)
            }
        } else {
            window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 1050, height: 700),
                styleMask: [.titled, .closable, .miniaturizable, .resizable],
                backing: .buffered,
                defer: false
            )
            window.title = "Claude Dashboard"
            window.center()
            window.minSize = NSSize(width: 1050, height: 450)
            window.isReleasedWhenClosed = false
            window.delegate = self
            dashboardWindow = window
        }

        window.contentView = NSHostingView(rootView: contentView)
        resizeWindowToFitContent(window: window, viewModel: viewModel)
        navigationCancellable = viewModel.$navigation
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in
                Task { @MainActor [weak self] in
                    guard let self, let win = self.dashboardWindow, let vm = self.currentViewModel else { return }
                    self.resizeWindowToFitContent(window: win, viewModel: vm)
                }
            }
        NSApp.setActivationPolicy(.regular)  // show dock icon
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @MainActor private func resizeWindowToFitContent(window: NSWindow, viewModel: DashboardViewModel) {
        let height = idealContentHeight(for: viewModel, in: window)
        let width = max(window.frame.width, 1050)
        let old = window.frame
        window.setFrame(
            NSRect(x: old.origin.x, y: old.origin.y + old.height - height, width: width, height: height),
            display: true, animate: false
        )
    }

    @MainActor private func idealContentHeight(for viewModel: DashboardViewModel, in window: NSWindow) -> CGFloat {
        let n = viewModel.accountStates.count
        let screenMax = (window.screen?.visibleFrame.height ?? 1200) - 80
        let raw: CGFloat
        switch viewModel.navigation {
        case .overview:
            // header 50 + chart container (toolbar 36 + chart 300) + 2 dividers
            // + legend rows (accounts + 1 Total) * 28 + separator + padding
            raw = 50 + 336 + 2 + CGFloat(max(1, n + 1)) * 28 + 12 + 16
        case .dashboard:
            // header 50 + grid rows * (card ~320 + spacing 12) + outer padding 24
            let cols = 2  // adaptive(min: 440) at 1050 width → 2 columns
            let rows = max(1, (n + cols - 1) / cols)
            raw = 50 + CGFloat(rows) * (320 + 12) + 24
        case .accountDetail:
            raw = 720
        }
        return max(450, min(raw, screenMax))
    }
}

/// Wrapper that handles first-time setup sheet via SwiftUI
struct DashboardWindowWrapper: View {
    @ObservedObject var viewModel: DashboardViewModel
    let showSetupOnAppear: Bool
    @State private var showingSetup = false

    var body: some View {
        DashboardWindow(viewModel: viewModel, onAddAccount: { showingSetup = true })
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
