import XCTest
@testable import ClaudeDashboard

/// Records calls and lets a test choose the status a register leaves behind or an error to throw.
private final class FakeLoginItemService: LoginItemService {
    var status: LoginItemStatus
    var statusAfterRegister: LoginItemStatus = .enabled
    var registerError: Error?
    private(set) var registerCalls = 0
    private(set) var unregisterCalls = 0

    init(status: LoginItemStatus) { self.status = status }

    func register() throws {
        registerCalls += 1
        if let registerError { throw registerError }
        status = statusAfterRegister
    }

    func unregister() throws {
        unregisterCalls += 1
        status = .notRegistered
    }
}

@MainActor
final class LaunchAtLoginControllerTests: XCTestCase {
    private let boom = NSError(domain: "t", code: 1, userInfo: [NSLocalizedDescriptionKey: "boom"])

    func testDefaultsOff() {
        let c = LaunchAtLoginController(service: FakeLoginItemService(status: .notRegistered))
        XCTAssertFalse(c.isOn)
        XCTAssertNil(c.errorMessage)
    }

    func testTurnOnRegisters() {
        let svc = FakeLoginItemService(status: .notRegistered)
        let c = LaunchAtLoginController(service: svc)
        c.setEnabled(true)
        XCTAssertEqual(svc.registerCalls, 1)
        XCTAssertTrue(c.isOn)
    }

    func testTurnOffUnregisters() {
        let svc = FakeLoginItemService(status: .enabled)
        let c = LaunchAtLoginController(service: svc)
        c.setEnabled(false)
        XCTAssertEqual(svc.unregisterCalls, 1)
        XCTAssertFalse(c.isOn)
    }

    func testTurnOffWhenNotRegisteredSkipsUnregister() {
        let svc = FakeLoginItemService(status: .notRegistered)
        let c = LaunchAtLoginController(service: svc)
        c.setEnabled(false)
        XCTAssertEqual(svc.unregisterCalls, 0)
        XCTAssertNil(c.errorMessage)
    }

    func testRegisterFailureKeepsOffAndShowsError() {
        let svc = FakeLoginItemService(status: .notRegistered)
        svc.registerError = boom
        let c = LaunchAtLoginController(service: svc)
        c.setEnabled(true)
        XCTAssertFalse(c.isOn)
        XCTAssertTrue(c.errorMessage?.contains("boom") == true)
    }

    func testRequiresApproval() {
        let svc = FakeLoginItemService(status: .notRegistered)
        svc.statusAfterRegister = .requiresApproval
        let c = LaunchAtLoginController(service: svc)
        c.setEnabled(true)
        XCTAssertTrue(c.isOn)
        XCTAssertTrue(c.needsApproval)
    }

    func testRefreshPicksUpExternalChange() {
        let svc = FakeLoginItemService(status: .notRegistered)
        let c = LaunchAtLoginController(service: svc)
        svc.status = .enabled
        c.refresh()
        XCTAssertTrue(c.isOn)
    }

    func testSuccessClearsPreviousError() {
        let svc = FakeLoginItemService(status: .notRegistered)
        svc.registerError = boom
        let c = LaunchAtLoginController(service: svc)
        c.setEnabled(true)
        XCTAssertNotNil(c.errorMessage)
        svc.registerError = nil
        c.setEnabled(true)
        XCTAssertNil(c.errorMessage)
        XCTAssertTrue(c.isOn)
    }

    /// Coming back from System Settings (approval, or a change in Login Items) re-reads the list.
    func testRefreshesWhenAppBecomesActive() {
        let center = NotificationCenter()
        let svc = FakeLoginItemService(status: .requiresApproval)
        let c = LaunchAtLoginController(service: svc, notificationCenter: center)
        XCTAssertTrue(c.needsApproval)
        svc.status = .enabled
        center.post(name: NSApplication.didBecomeActiveNotification, object: nil)
        XCTAssertFalse(c.needsApproval)
        XCTAssertTrue(c.isOn)
    }

    func testLiveUnderTestsIsInMemory() {
        XCTAssertFalse(LaunchAtLoginController.live(isRunningTests: true).isOn)
    }
}
