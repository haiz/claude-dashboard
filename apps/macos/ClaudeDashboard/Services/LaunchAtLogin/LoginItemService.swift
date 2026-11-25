import Foundation
import ServiceManagement

/// Whether the app is in the user's Login Items, as macOS reports it.
enum LoginItemStatus: Equatable {
    case notRegistered, enabled, requiresApproval, notFound
}

/// The system's login-item list for this app. The live type is `SystemLoginItemService`;
/// tests use `InMemoryLoginItemService` so they never touch the real list.
///
///     let svc: LoginItemService = SystemLoginItemService()
///     if svc.status != .enabled { try svc.register() }
protocol LoginItemService: AnyObject {
    var status: LoginItemStatus { get }
    func register() throws
    func unregister() throws
}

/// `SMAppService.mainApp`: the app itself as a login item (macOS 13+).
final class SystemLoginItemService: LoginItemService {
    var status: LoginItemStatus {
        switch SMAppService.mainApp.status {
        case .notRegistered: return .notRegistered
        case .enabled: return .enabled
        case .requiresApproval: return .requiresApproval
        case .notFound: return .notFound
        @unknown default: return .notFound
        }
    }

    func register() throws { try SMAppService.mainApp.register() }
    func unregister() throws { try SMAppService.mainApp.unregister() }
}

/// A login-item list held in memory, used under XCTest.
///
///     let svc = InMemoryLoginItemService()   // status == .notRegistered
///     try svc.register()                     // status == .enabled
final class InMemoryLoginItemService: LoginItemService {
    var status: LoginItemStatus = .notRegistered
    func register() throws { status = .enabled }
    func unregister() throws { status = .notRegistered }
}
