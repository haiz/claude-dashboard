import XCTest
@testable import ClaudeDashboard

/// The 60 s refresh of a cheetah or rocket account (`DashboardViewModel+BurnPoll`).
/// Tests observe `burnPollTask`, never the timer itself.
@MainActor
final class BurnPollTests: XCTestCase {

    private var tempDir: URL!
    private var defaultsSuiteName: String!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("BurnPollTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defaultsSuiteName = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: defaultsSuiteName)!
        // `autoRefreshEnabled` and `showBurnAnimals` live in the shared test defaults.
        AppDefaults.shared.removeObject(forKey: "autoRefreshEnabled")
        AppDefaults.shared.removeObject(forKey: AppDefaults.showBurnAnimalsKey)
    }

    override func tearDown() {
        AppDefaults.shared.removeObject(forKey: "autoRefreshEnabled")
        AppDefaults.shared.removeObject(forKey: AppDefaults.showBurnAnimalsKey)
        try? FileManager.default.removeItem(at: tempDir)
        StoreFixture.destroy(suite: defaultsSuiteName)
        super.tearDown()
    }

    // MARK: - Helpers

    /// Isolated stores, no browser cookies, no network: nothing here reaches real data.
    private func makeViewModel() -> DashboardViewModel {
        let detector = ClaudeCodeAccountDetector(
            fileURL: tempDir.appendingPathComponent(".claude.json-\(UUID().uuidString)"))
        let pingController = PingController(
            service: FakePinger(),
            logStore: PingLogStore(dbPath: tempDir.appendingPathComponent("pings.sqlite").path),
            defaults: defaults)
        let vm = DashboardViewModel(
            accountStore: AccountStore(defaults: defaults),
            logStore: UsageLogStore(dbPath: tempDir.appendingPathComponent("usage.sqlite").path),
            ccDetector: detector,
            cookieProvider: { _, _ in ChromeCookieResult(sessionKey: nil, orgId: nil) },
            autoSwitchLogStore: AutoSwitchLogStore(
                dbPath: tempDir.appendingPathComponent("auto_switch.sqlite").path),
            pingController: pingController)
        addTeardownBlock { vm.burnPollTask?.cancel() }
        return vm
    }

    private func makeState(level: Int?, status: AccountStatus = .active,
                           error: String? = nil) -> AccountUsageState {
        let account = Account(id: UUID(), name: "A", email: nil, chromeProfilePath: "Profile 1",
                              chromeProfileName: nil, orgId: "org", accountUuid: nil, plan: .max5x,
                              lastSynced: nil, status: status, isPinned: false, source: .browser)
        let result: BurnRateResult?
        switch level {
        case BurnRateResult.rocketLevel?:
            result = BurnRateResult.from(ratePerMinute: 10, sustainedSeconds: 0, utilization: 50)
        case 5?: result = BurnRateResult.fromProjectedTime(600)
        case 4?: result = BurnRateResult.fromProjectedTime(3000)
        default: result = nil
        }
        return AccountUsageState(id: account.id, account: account, error: error,
                                 burnRates: BurnRates(fiveHour: result))
    }

    // MARK: - burnFastIDs

    func testBurnFastIDs_onlyCheetahOrRocketActiveErrorFree() {
        let vm = makeViewModel()
        let cheetah = makeState(level: 5)
        let rocket = makeState(level: BurnRateResult.rocketLevel)
        vm.accountStates = [
            cheetah, rocket,
            makeState(level: 4),
            makeState(level: nil),
            makeState(level: 5, status: .expired),
            makeState(level: BurnRateResult.rocketLevel, error: "HTTP 500"),
        ]
        XCTAssertEqual(rocket.burnRates?.fiveHour?.level, 6)
        XCTAssertEqual(vm.burnFastIDs(), [cheetah.id, rocket.id])
    }

    func testBurnFastIDs_emptyWhenAutoRefreshOff() {
        let vm = makeViewModel()
        vm.autoRefreshEnabled = false
        vm.accountStates = [makeState(level: 5)]
        XCTAssertEqual(vm.burnFastIDs(), [])
    }

    func testBurnFastIDs_independentOfShowBurnAnimals() {
        let vm = makeViewModel()
        vm.showBurnAnimals = false
        let cheetah = makeState(level: 5)
        vm.accountStates = [cheetah]
        XCTAssertEqual(vm.burnFastIDs(), [cheetah.id])
    }

    // MARK: - updateBurnPoll

    func testUpdateBurnPoll_startsKeepsRearmsAndStops() {
        let vm = makeViewModel()
        vm.updateBurnPoll(afterFullRefresh: false)
        XCTAssertNil(vm.burnPollTask, "no fast account, no poll")

        vm.accountStates = [makeState(level: 5)]
        vm.updateBurnPoll(afterFullRefresh: false)
        let first = vm.burnPollTask
        XCTAssertNotNil(first, "a cheetah starts the poll")

        vm.updateBurnPoll(afterFullRefresh: false)
        XCTAssertTrue(vm.burnPollTask == first, "the poll's own refresh keeps the running task")

        vm.updateBurnPoll(afterFullRefresh: true)
        XCTAssertNotNil(vm.burnPollTask)
        XCTAssertFalse(vm.burnPollTask == first, "a full refresh re-arms the poll")
        XCTAssertTrue(first?.isCancelled ?? false, "the old task is cancelled")

        vm.accountStates = [makeState(level: 4)]
        vm.updateBurnPoll(afterFullRefresh: false)
        XCTAssertNil(vm.burnPollTask, "no fast account left, the poll stops")
    }

    /// `burnPollFetching` stands in for the loop being inside its `refreshAll(only:)`.
    func testFullRefreshDuringPollFetch_keepsTheTaskUncancelled() {
        let vm = makeViewModel()
        vm.accountStates = [makeState(level: 5)]
        vm.updateBurnPoll(afterFullRefresh: true)
        let task = vm.burnPollTask
        let token = vm.burnPollToken
        vm.burnPollFetching = true
        vm.updateBurnPoll(afterFullRefresh: true)
        XCTAssertTrue(vm.burnPollTask == task, "no re-arm mid-fetch")
        XCTAssertFalse(task?.isCancelled ?? true, "the in-flight fetch is not cancelled")
        XCTAssertEqual(vm.burnPollToken, token, "the loop keeps running after its fetch")
    }

    func testAutoRefreshOffDuringPollFetch_doesNotCancelButEndsTheLoop() {
        let vm = makeViewModel()
        vm.accountStates = [makeState(level: 5)]
        vm.updateBurnPoll(afterFullRefresh: true)
        let task = vm.burnPollTask
        addTeardownBlock { task?.cancel() }
        vm.burnPollFetching = true
        vm.autoRefreshEnabled = false
        XCTAssertFalse(task?.isCancelled ?? true, "the in-flight fetch is not cancelled")
        XCTAssertNil(vm.burnPollTask)
        XCTAssertNil(vm.burnPollToken, "the loop's token no longer matches, so it exits after the fetch")
    }

    func testTurningAutoRefreshOffStopsThePoll() {
        let vm = makeViewModel()
        vm.accountStates = [makeState(level: BurnRateResult.rocketLevel)]
        vm.updateBurnPoll(afterFullRefresh: true)
        let task = vm.burnPollTask
        XCTAssertNotNil(task)
        vm.autoRefreshEnabled = false
        XCTAssertNil(vm.burnPollTask)
        XCTAssertTrue(task?.isCancelled ?? false)
    }
}
