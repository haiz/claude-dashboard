import XCTest
@testable import ClaudeDashboard

/// The two Claude Desktop switch settings: both default off and persist to `AppDefaults.shared`
/// (the test suite under XCTest, never the user's domain).
@MainActor
final class DesktopSwitchSettingsTests: XCTestCase {
    private var tempDir: URL!
    private var defaultsSuiteName: String!
    private let keys = ["switchDesktopEnabled", "autoSwitchDesktopEnabled"]

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("DesktopSwitchSettingsTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defaultsSuiteName = StoreFixture.makeSuiteName()
        keys.forEach { AppDefaults.shared.removeObject(forKey: $0) }
    }

    override func tearDown() {
        keys.forEach { AppDefaults.shared.removeObject(forKey: $0) }
        try? FileManager.default.removeItem(at: tempDir)
        StoreFixture.destroy(suite: defaultsSuiteName)
        super.tearDown()
    }

    private func makeViewModel() -> DashboardViewModel {
        let defaults = UserDefaults(suiteName: defaultsSuiteName)!
        return DashboardViewModel(
            accountStore: AccountStore(defaults: defaults),
            logStore: UsageLogStore(dbPath: tempDir.appendingPathComponent("usage.sqlite").path),
            autoSwitchLogStore: AutoSwitchLogStore(
                dbPath: tempDir.appendingPathComponent("auto_switch.sqlite").path),
            pingController: PingController(
                service: PingService(),
                logStore: PingLogStore(dbPath: tempDir.appendingPathComponent("pings.sqlite").path),
                defaults: defaults))
    }

    func testDesktopSettingsDefaultOffAndPersist() {
        let vm = makeViewModel()
        XCTAssertFalse(vm.switchDesktopEnabled)
        XCTAssertFalse(vm.autoSwitchDesktopEnabled)

        vm.switchDesktopEnabled = true
        vm.autoSwitchDesktopEnabled = true
        XCTAssertTrue(AppDefaults.shared.bool(forKey: "switchDesktopEnabled"))
        XCTAssertTrue(AppDefaults.shared.bool(forKey: "autoSwitchDesktopEnabled"))

        let reloaded = makeViewModel()
        XCTAssertTrue(reloaded.switchDesktopEnabled)
        XCTAssertTrue(reloaded.autoSwitchDesktopEnabled)

        reloaded.resetSettingsToDefaults()
        XCTAssertFalse(reloaded.switchDesktopEnabled)
        XCTAssertFalse(reloaded.autoSwitchDesktopEnabled)
    }
}
