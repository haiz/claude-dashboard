import XCTest
@testable import ClaudeDashboard

@MainActor
final class AutoSwitchControllerTests: XCTestCase {

    private var suite: String!
    private var settings: AutoSwitchSettings!
    private var clock = Date(timeIntervalSince1970: 1_800_000_000)
    private var controller: AutoSwitchController!
    private var logPath: String!

    override func setUp() async throws {
        try await super.setUp()
        suite = StoreFixture.makeSuiteName()
        settings = AutoSwitchSettings(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        settings.isEnabled = true
        logPath = NSTemporaryDirectory() + "auto_switch_logs_\(UUID().uuidString).db"
        controller = AutoSwitchController(settings: settings, logStore: AutoSwitchLogStore(dbPath: logPath),
                                          now: { [unowned self] in self.clock },
                                          pickIndex: { _ in 0 })
    }

    override func tearDown() async throws {
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(atPath: logPath)
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

    func testActiveLoginFailurePausesAutoSwitchInsteadOfTryingEachCandidate() async {
        let active = account("a", five: 96, seven: 10, active: true)
        let first = account("b", five: 10, seven: 10)
        let second = account("c", five: 10, seven: 10)
        var tried: [UUID] = []
        let perform: (UUID) async throws -> Void = { tried.append($0); throw SwitchError.activeAccountUnknown }

        _ = await controller.evaluate([active, first, second], perform: perform)
        clock += 60
        _ = await controller.evaluate([active, first, second], perform: perform)
        XCTAssertEqual(tried, [first.id], "the current login is the problem, not the target")
        XCTAssertEqual(controller.lastEvent?.isFailure, true)

        clock += AutoSwitchController.failureCooldown
        _ = await controller.evaluate([active, first, second], perform: perform)
        XCTAssertEqual(tried.count, 2)
    }

    func testShouldFastPollIgnoresStaleActiveUsage() {
        var active = account("a", five: 93, seven: 10, active: true)
        active.isFresh = false
        XCTAssertFalse(controller.shouldFastPoll([active]))
    }

    func testPriorityKeptAfterSwitch() async {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 20, fiveResets: nil)
        settings.setPriority(AutoSwitchPriority(accountId: ticked.id, baseline: clock - 600))
        let result = await controller.evaluate([active, ticked]) { _ in }
        XCTAssertEqual(result, ticked.id)
        XCTAssertEqual(settings.priority?.accountId, ticked.id)
        XCTAssertEqual(controller.lastEvent?.kind, .switched(.priorityReset))
    }

    func testActiveTickedAccountKeepsTickAndFollowsItsWindowEvenWhileDisabled() async {
        settings.isEnabled = false
        let home = account("h", five: 40, seven: 20, fiveResets: clock + 3600, active: true)
        settings.setPriority(AutoSwitchPriority(accountId: home.id, baseline: clock - 9000))
        _ = await controller.evaluate([home, account("b", five: 0, seven: 10)]) { _ in }
        XCTAssertEqual(settings.priority, AutoSwitchPriority(accountId: home.id, baseline: clock + 3600))
    }

    func testActiveTickedAccountBeforeItsWindowStartsWaitsFiveHours() async {
        let home = account("h", five: 0, seven: 20, fiveResets: nil, active: true)
        settings.setPriority(AutoSwitchPriority(accountId: home.id, baseline: nil))
        _ = await controller.evaluate([home]) { _ in }
        XCTAssertEqual(settings.priority?.baseline, clock + 5 * 3600)
    }

    func testStaleActiveTickedAccountKeepsItsBaseline() async {
        var home = account("h", five: 40, seven: 20, fiveResets: clock + 3600, active: true)
        home.isFresh = false
        settings.setPriority(AutoSwitchPriority(accountId: home.id, baseline: clock - 9000))
        _ = await controller.evaluate([home]) { _ in }
        XCTAssertEqual(settings.priority?.baseline, clock - 9000)
    }

    func testLeavesTheHomeAccountAtItsLimitAndComesBackAfterItsReset() async {
        let homeId = UUID()
        func home(five: Double, resets: Date?, active: Bool) -> AutoSwitchAccount {
            AutoSwitchAccount(id: homeId, displayName: "h",
                              usage: UsageData(fiveHour: UsageLimit(utilization: five, resetsAt: resets),
                                               sevenDay: UsageLimit(utilization: 20, resetsAt: clock + 86_400)),
                              availability: active ? .active : .ready, isUsable: true, isActive: active)
        }
        let otherId = UUID()
        func other(active: Bool) -> AutoSwitchAccount {
            AutoSwitchAccount(id: otherId, displayName: "o",
                              usage: UsageData(fiveHour: UsageLimit(utilization: 10, resetsAt: clock + 3600),
                                               sevenDay: UsageLimit(utilization: 20, resetsAt: clock + 86_400)),
                              availability: active ? .active : .ready, isUsable: true, isActive: active)
        }
        let resets = clock + 1800
        settings.setPriority(AutoSwitchPriority(accountId: homeId, baseline: nil))

        let left = await controller.evaluate([home(five: 96, resets: resets, active: true), other(active: false)]) { _ in }
        XCTAssertEqual(left, otherId)
        XCTAssertEqual(settings.priority, AutoSwitchPriority(accountId: homeId, baseline: resets))

        let early = await controller.evaluate([home(five: 96, resets: resets, active: false), other(active: true)]) { _ in }
        XCTAssertNil(early, "its window has not reset yet")

        clock = resets + 60
        let back = await controller.evaluate([home(five: 0, resets: nil, active: false), other(active: true)]) { _ in }
        XCTAssertEqual(back, homeId)
        XCTAssertEqual(settings.priority?.accountId, homeId)
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

    // MARK: - Switch Log

    private func logged() async -> [AutoSwitchLogEntry] {
        await controller.logStore.fetch(limit: 50).reversed()
    }

    func testSwitchIsLoggedWithTriggerTierAndTargetUsage() async {
        let active = account("a@x.com", five: 96, seven: 10, active: true)
        let target = account("b@x.com", five: 10, seven: 99.5)
        _ = await controller.evaluate([active, target]) { _ in }

        let rows = await logged()
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows.first?.outcome, .switched)
        XCTAssertEqual(rows.first?.date, clock)
        XCTAssertEqual(rows.first?.fromName, "a@x.com")
        XCTAssertEqual(rows.first?.toName, "b@x.com")
        XCTAssertEqual(rows.first?.reason, "5h 96% >= 95%, tier 3")
        XCTAssertEqual(rows.first?.detail, "b@x.com: 5h 10%, 7d 100%")
    }

    func testFailureIsLoggedWithTheError() async {
        let active = account("a", five: 96, seven: 99, active: true)
        let target = account("b", five: 10, seven: 10)
        _ = await controller.evaluate([active, target]) { _ in throw SwitchError.loginExpired }

        let rows = await logged()
        XCTAssertEqual(rows.map(\.outcome), [.failed])
        XCTAssertEqual(rows.first?.reason, "5h 96% >= 95%, 7d 99% >= 99%, tier 1")
        XCTAssertEqual(rows.first?.detail, "it needs a browser sign-in first")
    }

    func testPrioritySwitchIsLoggedAsTheReset() async {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 20, fiveResets: nil)
        settings.setPriority(AutoSwitchPriority(accountId: ticked.id, baseline: clock - 600))
        _ = await controller.evaluate([active, ticked]) { _ in }

        let rows = await logged()
        XCTAssertEqual(rows.first?.reason, "Ticked account's 5h window reset")
    }

    func testNoCandidateIsLoggedOncePerStretch() async {
        let active = account("a", five: 96, seven: 10, active: true)
        let notReady = account("b", five: 0, seven: 10, availability: .notCaptured)
        _ = await controller.evaluate([active, notReady]) { _ in }
        clock += 60
        _ = await controller.evaluate([active, notReady]) { _ in }
        let first = await logged()
        XCTAssertEqual(first.map(\.outcome), [.noCandidate], "a repeat is not logged again")

        clock += 60
        _ = await controller.evaluate([account("a", five: 50, seven: 10, active: true), notReady]) { _ in }
        clock += 60
        _ = await controller.evaluate([active, notReady]) { _ in }
        let rows = await logged()
        XCTAssertEqual(rows.map(\.outcome), [.noCandidate, .noCandidate], "a new stretch is logged")
        XCTAssertEqual(rows.first?.reason, "5h 96% >= 95%")
        XCTAssertNil(rows.first?.toName)
    }

    func testNothingIsLoggedBelowThresholdOrWhileDisabled() async {
        let target = account("b", five: 10, seven: 10)
        _ = await controller.evaluate([account("a", five: 50, seven: 10, active: true), target]) { _ in }
        settings.isEnabled = false
        _ = await controller.evaluate([account("a", five: 99, seven: 10, active: true), target]) { _ in }
        let rows = await logged()
        XCTAssertEqual(rows, [])
    }
}
