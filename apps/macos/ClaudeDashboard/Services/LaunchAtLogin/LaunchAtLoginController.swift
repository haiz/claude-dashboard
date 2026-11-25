import AppKit
import ServiceManagement

/// The "Open at login" switch in Settings > General. Holds no stored setting: `isOn` is read
/// from the login-item list each `refresh()`, so a change made in System Settings > General >
/// Login Items shows up here; it also refreshes whenever the app becomes active again. Off by default because nothing registers until the user turns it on.
///
///     let c = LaunchAtLoginController.live(isRunningTests: AppDefaults.isRunningTests())
///     c.setEnabled(true)        // registers; isOn == true, or errorMessage set
///     c.refresh()               // re-read after the user may have changed Login Items
@MainActor
final class LaunchAtLoginController: ObservableObject {
    /// Registered, whether or not macOS still wants the user's approval.
    @Published private(set) var isOn = false
    /// Registered but waiting for approval in System Settings > General > Login Items.
    @Published private(set) var needsApproval = false
    /// The last register/unregister failure; nil after a success.
    @Published private(set) var errorMessage: String?

    private let service: LoginItemService
    private var activeObserver: NSObjectProtocol?

    init(service: LoginItemService, notificationCenter: NotificationCenter = .default) {
        self.service = service
        refresh()
        activeObserver = notificationCenter.addObserver(
            forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.refresh() }
        }
    }

    /// The real login-item list, or an in-memory one under XCTest.
    static func live(isRunningTests: Bool) -> LaunchAtLoginController {
        LaunchAtLoginController(service: isRunningTests ? InMemoryLoginItemService() : SystemLoginItemService())
    }

    func refresh() {
        let status = service.status
        isOn = status == .enabled || status == .requiresApproval
        needsApproval = status == .requiresApproval
    }

    /// Registers or unregisters. Does nothing when already in that state (unregistering an
    /// app that is not registered throws).
    func setEnabled(_ on: Bool) {
        refresh()
        guard on != isOn else { return }
        do {
            if on { try service.register() } else { try service.unregister() }
            errorMessage = nil
        } catch {
            errorMessage = "Could not change the login item: \(error.localizedDescription)"
        }
        refresh()
    }

    /// Opens System Settings > General > Login Items. No-op for the in-memory list.
    func openLoginItems() {
        guard service is SystemLoginItemService else { return }
        SMAppService.openSystemSettingsLoginItems()
    }
}
