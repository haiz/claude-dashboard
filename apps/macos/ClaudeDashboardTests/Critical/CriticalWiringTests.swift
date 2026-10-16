import XCTest
@testable import ClaudeDashboard

@MainActor
final class CriticalWiringTests: XCTestCase {
    private var suite: String!
    private var defaults: UserDefaults!
    private var logPath: String!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suite)
        logPath = NSTemporaryDirectory() + "usage_\(UUID().uuidString).db"
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(atPath: logPath)
        super.tearDown()
    }

    /// An empty usage log keeps X at the default 16 whatever the hourly job does.
    private func makeViewModel() -> DashboardViewModel {
        DashboardViewModel(accountStore: AccountStore(defaults: defaults),
                           logStore: UsageLogStore(dbPath: logPath),
                           criticalSettings: CriticalSettings(defaults: defaults))
    }

    private func state(seven: Double, hoursLeft: Double, error: String? = nil) -> AccountUsageState {
        let account = Account(id: UUID(), name: "a", email: "a@x.com", chromeProfilePath: "Profile 1",
                              chromeProfileName: nil, orgId: nil, accountUuid: nil, plan: .max5x,
                              lastSynced: nil, status: .active, isPinned: false, source: .browser)
        let usage = UsageData(fiveHour: UsageLimit(utilization: 0, resetsAt: nil),
                              sevenDay: UsageLimit(utilization: seven, resetsAt: Date().addingTimeInterval(hoursLeft * 3600)))
        return AccountUsageState(id: account.id, account: account, usage: usage, error: error)
    }

    func testTenHoursLeftAtSixtyIsHighCritical() {
        let vm = makeViewModel()
        // X default 16, high pace 80: 2 windows x 12.8 = 25.6 -> 85.6.
        XCTAssertEqual(vm.criticalAssessment(for: state(seven: 60, hoursLeft: 10)).level, .high)
        XCTAssertEqual(vm.criticalBadge(for: state(seven: 60, hoursLeft: 10))?.level, .high)
    }

    func testBusyAccountIsNotCritical() {
        let vm = makeViewModel()
        XCTAssertEqual(vm.criticalAssessment(for: state(seven: 30, hoursLeft: 100)), .none)
        XCTAssertNil(vm.criticalBadge(for: state(seven: 30, hoursLeft: 100)))
    }

    func testStaleUsageIsNotCritical() {
        let vm = makeViewModel()
        XCTAssertEqual(vm.criticalAssessment(for: state(seven: 60, hoursLeft: 10, error: "HTTP 500")), .none)
    }

    func testAutoSwitchAccountsCarryTheAssessment() {
        let vm = makeViewModel()
        let s = state(seven: 60, hoursLeft: 10)
        vm.accountStates = [s]
        XCTAssertEqual(vm.autoSwitchAccounts.first?.critical.level, .high)
    }
}
