import XCTest
@testable import ClaudeDashboard

@MainActor
final class AutoSwitchControllerTests: XCTestCase {

    private var suite: String!
    private var settings: AutoSwitchSettings!
    private var clock = Date(timeIntervalSince1970: 1_800_000_000)
    private var controller: AutoSwitchController!

    override func setUp() async throws {
        try await super.setUp()
        suite = StoreFixture.makeSuiteName()
        settings = AutoSwitchSettings(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        settings.isEnabled = true
        controller = AutoSwitchController(settings: settings, now: { [unowned self] in self.clock },
                                          pickIndex: { _ in 0 })
    }

    override func tearDown() async throws {
        StoreFixture.destroy(suite: suite)
        try await super.tearDown()
    }

    private func account(_ name: String, five: Double, seven: Double, fiveResets: Date? = nil,
                         availability: SwitchAvailability = .ready, active: Bool = false) -> AutoSwitchAccount {
        AutoSwitchAccount(id: UUID(), displayName: name,
                          usage: UsageData(fiveHour: UsageLimit(utilization: five, resetsAt: fiveResets),
                                           sevenDay: UsageLimit(utilization: seven, resetsAt: clock + 86_400)),
                          availability: active ? .active : availability, isUsable: true, isActive: active)
    }

    func testDisabledDoesNothing() async {
        settings.isEnabled = false
        let accounts = [account("a", five: 99, seven: 10, active: true), account("b", five: 0, seven: 10)]
        var calls = 0
        let result = await controller.evaluate(accounts) { _ in calls += 1 }
        XCTAssertNil(result)
        XCTAssertEqual(calls, 0)
    }

    func testSwitchSetsLockAndEvent() async {
        let active = account("a", five: 96, seven: 10, active: true)
        let target = account("b@x.com", five: 10, seven: 99.5)   // tier 3: at the 7d threshold
        var performed: [UUID] = []
        let result = await controller.evaluate([active, target]) { performed.append($0) }
        XCTAssertEqual(result, target.id)
        XCTAssertEqual(performed, [target.id])
        XCTAssertEqual(controller.lock?.accountId, target.id)
        XCTAssertNotNil(controller.lock?.sevenDay)
        XCTAssertNil(controller.lock?.fiveHour)
        XCTAssertEqual(controller.lastEvent?.kind, .switched(.threshold(tier: 3)))
        XCTAssertEqual(controller.lastEvent?.text, "Auto-switched to b@x.com")
        controller.clearLock()
        XCTAssertNil(controller.lock)
    }

    func testFailureStartsCooldown() async {
        let active = account("a", five: 96, seven: 10, active: true)
        let target = account("b", five: 10, seven: 10)
        var calls = 0
        let perform: (UUID) async throws -> Void = { _ in calls += 1; throw SwitchError.loginExpired }

        _ = await controller.evaluate([active, target], perform: perform)
        XCTAssertEqual(calls, 1)
        XCTAssertEqual(controller.lastEvent?.isFailure, true)

        clock += 60
        _ = await controller.evaluate([active, target], perform: perform)
        XCTAssertEqual(calls, 1, "cooling down: not retried a minute later")
        XCTAssertEqual(controller.lastEvent?.kind, .noCandidate)

        clock += AutoSwitchController.failureCooldown
        _ = await controller.evaluate([active, target], perform: perform)
        XCTAssertEqual(calls, 2)
    }

    func testPriorityClearedAfterSwitch() async {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 20, fiveResets: nil)
        settings.setPriority(AutoSwitchPriority(accountId: ticked.id, baseline: clock - 600))
        let result = await controller.evaluate([active, ticked]) { _ in }
        XCTAssertEqual(result, ticked.id)
        XCTAssertNil(settings.priority)
        XCTAssertEqual(controller.lastEvent?.kind, .switched(.priorityReset))
    }

    func testPriorityDroppedWhenItsAccountIsGoneEvenWhileDisabled() async {
        settings.isEnabled = false
        settings.setPriority(AutoSwitchPriority(accountId: UUID(), baseline: nil))
        _ = await controller.evaluate([account("a", five: 10, seven: 10, active: true)]) { _ in }
        XCTAssertNil(settings.priority)
    }

    func testNoCandidateEvent() async {
        let active = account("a", five: 96, seven: 10, active: true)
        let notReady = account("b", five: 0, seven: 10, availability: .notCaptured)
        var calls = 0
        _ = await controller.evaluate([active, notReady]) { _ in calls += 1 }
        XCTAssertEqual(calls, 0)
        XCTAssertEqual(controller.lastEvent?.text, "No account to switch to")
    }

    func testShouldFastPollFollowsEnabled() {
        let accounts = [account("a", five: 92, seven: 10, active: true)]
        XCTAssertTrue(controller.shouldFastPoll(accounts))
        settings.isEnabled = false
        XCTAssertFalse(controller.shouldFastPoll(accounts))
        settings.isEnabled = true
        XCTAssertFalse(controller.shouldFastPoll([account("a", five: 50, seven: 10, active: true)]))
    }
}
