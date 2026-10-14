import XCTest
@testable import ClaudeDashboard

@MainActor
final class PingControllerTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!
    private var dbPath: String!
    private var clock = Date(timeIntervalSince1970: 1_800_000_000)
    private let id = UUID()

    override func setUp() {
        super.setUp()
        suiteName = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suiteName)!
        dbPath = NSTemporaryDirectory() + "ping_ctrl_\(UUID().uuidString).db"
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suiteName)
        try? FileManager.default.removeItem(atPath: dbPath)
        super.tearDown()
    }

    private func makeController(_ service: Pinging, autoPing: Bool = true) -> PingController {
        defaults.set(autoPing, forKey: PingController.enabledKey)
        return PingController(service: service, logStore: PingLogStore(dbPath: dbPath),
                              defaults: defaults, now: { [unowned self] in self.clock })
    }

    private var target: PingTarget { PingTarget(accountId: id, orgId: "org-1", sessionKey: "sk-1") }

    private func usage(fiveHour: Date?, sevenDay: Date?) -> UsageData {
        UsageData(fiveHour: UsageLimit(utilization: 0, resetsAt: fiveHour),
                  sevenDay: UsageLimit(utilization: 0, resetsAt: sevenDay))
    }

    private var running: Date { clock.addingTimeInterval(3600) }

    // MARK: - The rule

    func testWindowNotRunningWhenEitherResetIsNil() {
        XCTAssertTrue(PingController.isWindowNotRunning(usage(fiveHour: nil, sevenDay: running)))
        XCTAssertTrue(PingController.isWindowNotRunning(usage(fiveHour: running, sevenDay: nil)))
        XCTAssertTrue(PingController.isWindowNotRunning(usage(fiveHour: nil, sevenDay: nil)))
        XCTAssertFalse(PingController.isWindowNotRunning(usage(fiveHour: running, sevenDay: running)))
    }

    func testClaimsAnActiveAccountWhoseWindowIsNotRunning() {
        let controller = makeController(FakePinger())
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active,
                                               freshUsage: usage(fiveHour: nil, sevenDay: running)))
    }

    func testNeverClaimsWithoutFreshUsage() {
        let controller = makeController(FakePinger())
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active, freshUsage: nil))
    }

    func testNeverClaimsWhenAutoPingIsOff() {
        let controller = makeController(FakePinger(), autoPing: false)
        XCTAssertFalse(controller.isAutoPingEnabled)
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: nil, sevenDay: nil)))
    }

    func testAutoPingDefaultsToOff() {
        let controller = PingController(service: FakePinger(), logStore: PingLogStore(dbPath: dbPath),
                                        defaults: defaults)
        XCTAssertFalse(controller.isAutoPingEnabled)
    }

    func testTogglePersists() {
        let controller = makeController(FakePinger(), autoPing: false)
        controller.isAutoPingEnabled = true
        XCTAssertTrue(defaults.bool(forKey: PingController.enabledKey))
    }

    func testNeverClaimsANonActiveAccount() {
        let controller = makeController(FakePinger())
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .expired,
                                                freshUsage: usage(fiveHour: nil, sevenDay: nil)))
    }

    func testNeverClaimsWhenBothWindowsRun() {
        let controller = makeController(FakePinger())
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: running, sevenDay: running)))
    }

    // MARK: - Latch

    func testLatchBlocksASecondClaimUntilBothWindowsRunAgain() async {
        let controller = makeController(FakePinger())
        let idle = usage(fiveHour: nil, sevenDay: nil)
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
        await controller.ping(target, trigger: .auto)

        // The API can still say null for a few seconds after a ping.
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
        // Only one window running is not enough to clear the latch.
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: running, sevenDay: nil)))

        // Both running: latch cleared; the next reset claims again.
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: running, sevenDay: running)))
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
    }

    func testAManualOkPingAlsoLatches() async {
        let controller = makeController(FakePinger())
        await controller.ping(target, trigger: .manual)
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: nil, sevenDay: nil)))
    }

    // MARK: - Failure cooldown

    func testFailedAutoPingRetriesOnlyAfterTenMinutes() async {
        let pinger = FakePinger(result: PingResult(outcome: .blocked, detail: nil))
        let controller = makeController(pinger)
        let idle = usage(fiveHour: nil, sevenDay: nil)
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
        await controller.ping(target, trigger: .auto)

        clock = clock.addingTimeInterval(PingController.failureCooldown - 1)
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))

        clock = clock.addingTimeInterval(1)
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
        // Claimed once; claiming again right away is blocked until the next failure sets a new cooldown.
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
    }

    // MARK: - In flight

    func testASecondPingWhileOneIsInFlightIsSkipped() async {
        let gated = GatedPinger()
        let controller = makeController(gated)
        let first = Task { await controller.ping(target, trigger: .manual) }
        for _ in 0..<1000 where gated.startedCount == 0 { await Task.yield() }
        XCTAssertEqual(gated.startedCount, 1)
        XCTAssertTrue(controller.inFlight.contains(id))
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: nil, sevenDay: nil)))

        let second = await controller.ping(target, trigger: .auto)
        XCTAssertNil(second)

        gated.release()
        let firstResult = await first.value
        XCTAssertEqual(firstResult?.outcome, .ok)
        XCTAssertFalse(controller.inFlight.contains(id))
        XCTAssertEqual(gated.startedCount, 1)
    }

    // MARK: - Log

    func testEachPingWritesOneLogRowAndUpdatesLatest() async {
        let pinger = FakePinger(result: PingResult(outcome: .failed, detail: "HTTP 429"))
        let controller = makeController(pinger)

        await controller.ping(target, trigger: .manual)

        let rows = await controller.logStore.fetch(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows[0].accountId, id)
        XCTAssertEqual(rows[0].trigger, .manual)
        XCTAssertEqual(rows[0].detail, "HTTP 429")
        XCTAssertEqual(controller.latest[id]?.outcome, .failed)
    }

    func testLoadLatestReadsTheLog() async {
        let store = PingLogStore(dbPath: dbPath)
        await store.insert(accountId: id, trigger: .auto, startedAt: clock, finishedAt: clock,
                           result: PingResult(outcome: .ok, detail: nil))
        let controller = PingController(service: FakePinger(), logStore: store, defaults: defaults)

        await controller.loadLatest(for: id)

        XCTAssertEqual(controller.latest[id]?.trigger, .auto)
    }
}
