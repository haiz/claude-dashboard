import XCTest
@testable import ClaudeDashboard

final class AutoSwitchPlannerTests: XCTestCase {

    private let now = Date(timeIntervalSince1970: 1_800_000_000)
    private let day: TimeInterval = 86_400
    private let base = AutoSwitchThresholds(fiveHour: 95, sevenDay: 99)

    private func account(_ name: String, five: Double, seven: Double,
                         fiveResets: Date? = nil, sevenResets: Date? = nil,
                         availability: SwitchAvailability? = .ready, usable: Bool = true,
                         active: Bool = false, hasUsage: Bool = true) -> AutoSwitchAccount {
        AutoSwitchAccount(
            id: UUID(), displayName: name,
            usage: hasUsage ? UsageData(fiveHour: UsageLimit(utilization: five, resetsAt: fiveResets),
                                        sevenDay: UsageLimit(utilization: seven, resetsAt: sevenResets)) : nil,
            availability: active ? .active : availability, isUsable: usable, isActive: active)
    }

    private func snapshot(_ accounts: [AutoSwitchAccount], lock: AutoSwitchLock? = nil,
                          priority: AutoSwitchPriority? = nil, excluded: Set<UUID> = []) -> AutoSwitchSnapshot {
        AutoSwitchSnapshot(accounts: accounts, thresholds: base, lock: lock,
                           priority: priority, excluded: excluded, now: now)
    }

    private func decide(_ s: AutoSwitchSnapshot, pick: (Int) -> Int = { _ in 0 }) -> AutoSwitchDecision {
        AutoSwitchPlanner.decide(s, pickIndex: pick)
    }

    // MARK: Trigger

    func testStaysBelowThreshold() {
        let active = account("a", five: 94, seven: 98, active: true)
        let other = account("b", five: 0, seven: 10)
        XCTAssertEqual(decide(snapshot([active, other])), .stay(.belowThreshold))
    }

    func testTriggersAtExactFiveHourThreshold() {
        let active = account("a", five: 95, seven: 10, active: true)
        let other = account("b", five: 0, seven: 10)
        XCTAssertEqual(decide(snapshot([active, other])), .switchTo(other.id, .threshold(tier: 1)))
    }

    func testTriggersAtExactSevenDayThreshold() {
        let active = account("a", five: 10, seven: 99, active: true)
        let other = account("b", five: 0, seven: 10)
        XCTAssertEqual(decide(snapshot([active, other])), .switchTo(other.id, .threshold(tier: 1)))
    }

    func testStaysWithoutActiveAccount() {
        let a = account("a", five: 99, seven: 10)
        let b = account("b", five: 0, seven: 10)
        XCTAssertEqual(decide(snapshot([a, b])), .stay(.noActiveAccount))
    }

    func testStaysWhenActiveHasNoUsage() {
        let active = account("a", five: 0, seven: 0, active: true, hasUsage: false)
        let b = account("b", five: 0, seven: 10)
        XCTAssertEqual(decide(snapshot([active, b])), .stay(.noActiveAccount))
    }

    func testStaysWhenActiveUsageIsStale() {
        var active = account("a", five: 99, seven: 10, active: true)
        active.isFresh = false
        let b = account("b", five: 0, seven: 10)
        XCTAssertEqual(decide(snapshot([active, b])), .stay(.noActiveAccount))
    }

    func testStaleCandidateIsSkipped() {
        let active = account("a", five: 96, seven: 10, active: true)
        var stale = account("b", five: 0, seven: 10)
        stale.isFresh = false
        XCTAssertEqual(decide(snapshot([active, stale])), .stay(.noCandidate))
    }

    func testWarningZoneEdges() {
        func usage(_ five: Double, _ seven: Double) -> UsageData {
            UsageData(fiveHour: UsageLimit(utilization: five, resetsAt: nil),
                      sevenDay: UsageLimit(utilization: seven, resetsAt: nil))
        }
        XCTAssertTrue(AutoSwitchPlanner.isInWarningZone(usage(92, 0), thresholds: base))
        XCTAssertFalse(AutoSwitchPlanner.isInWarningZone(usage(91.9, 0), thresholds: base))
        XCTAssertTrue(AutoSwitchPlanner.isInWarningZone(usage(0, 98), thresholds: base))
        XCTAssertFalse(AutoSwitchPlanner.isInWarningZone(usage(0, 97.9), thresholds: base))
    }

    func testToggleZoneEdges() {
        func usage(_ five: Double, _ seven: Double) -> UsageData {
            UsageData(fiveHour: UsageLimit(utilization: five, resetsAt: nil),
                      sevenDay: UsageLimit(utilization: seven, resetsAt: nil))
        }
        XCTAssertTrue(AutoSwitchPlanner.isInToggleZone(usage(80, 0), thresholds: base))
        XCTAssertFalse(AutoSwitchPlanner.isInToggleZone(usage(79.9, 0), thresholds: base))
        XCTAssertTrue(AutoSwitchPlanner.isInToggleZone(usage(0, 96), thresholds: base))
        XCTAssertFalse(AutoSwitchPlanner.isInToggleZone(usage(0, 95.9), thresholds: base))
    }

    // MARK: Candidates

    func testCandidateFiltersLeaveNoCandidate() {
        let active = account("a", five: 96, seven: 10, active: true)
        let notReady = account("b", five: 0, seven: 10, availability: .notCaptured)
        let expiredLogin = account("c", five: 0, seven: 10, availability: .loginExpired)
        let unusable = account("d", five: 0, seven: 10, usable: false)
        let exhausted5h = account("e", five: 100, seven: 10)
        let exhausted7d = account("f", five: 0, seven: 100)
        let noUsage = account("g", five: 0, seven: 0, hasUsage: false)
        let cooling = account("h", five: 0, seven: 10)
        let s = snapshot([active, notReady, expiredLogin, unusable, exhausted5h, exhausted7d, noUsage, cooling],
                         excluded: [cooling.id])
        XCTAssertEqual(decide(s), .stay(.noCandidate))
    }

    // MARK: Tier 1 score and group

    func testScore() {
        func usage(_ seven: Double, resets: Date?) -> UsageData {
            UsageData(fiveHour: UsageLimit(utilization: 0, resetsAt: nil),
                      sevenDay: UsageLimit(utilization: seven, resetsAt: resets))
        }
        XCTAssertEqual(AutoSwitchPlanner.score(usage(58, resets: now + day), now: now), 40, accuracy: 0.001)
        XCTAssertEqual(AutoSwitchPlanner.score(usage(8, resets: now + 6 * day), now: now), 15, accuracy: 0.001)
        XCTAssertEqual(AutoSwitchPlanner.score(usage(10, resets: nil), now: now), 0)
        XCTAssertEqual(AutoSwitchPlanner.score(usage(10, resets: now + 9 * day), now: now), 88.0 / 7, accuracy: 0.001)
        XCTAssertEqual(AutoSwitchPlanner.score(usage(10, resets: now - day), now: now), 88 / 0.25, accuracy: 0.001)
        XCTAssertEqual(AutoSwitchPlanner.score(usage(10, resets: now + 3600), now: now), 88 / 0.25, accuracy: 0.001)
        XCTAssertEqual(AutoSwitchPlanner.score(usage(98.5, resets: now + day), now: now), 0)
    }

    /// Time left weighs more than percent used: 6 days in at 50% beats 4 days in at 0%,
    /// and by more than the near-top group, so there is no coin flip.
    func testLateHalfUsedClearlyBeatsEarlierUnused() {
        let active = account("a", five: 96, seven: 10, active: true)
        let late = account("late", five: 0, seven: 50, sevenResets: now + day)        // 48
        let unused = account("unused", five: 0, seven: 0, sevenResets: now + 3 * day) // 32.7
        var sizes: [Int] = []
        let d = decide(snapshot([active, unused, late]), pick: { sizes.append($0); return $0 - 1 })
        XCTAssertEqual(d, .switchTo(late.id, .threshold(tier: 1)))
        XCTAssertEqual(sizes, [1])
    }

    func testFarAheadTopIsPicked() {
        let active = account("a", five: 96, seven: 10, active: true)
        let late = account("late", five: 0, seven: 58, sevenResets: now + day)       // 40
        let early = account("early", five: 0, seven: 8, sevenResets: now + 6 * day)  // 15
        var sizes: [Int] = []
        let d = decide(snapshot([active, early, late]), pick: { sizes.append($0); return $0 - 1 })
        XCTAssertEqual(d, .switchTo(late.id, .threshold(tier: 1)))
        XCTAssertEqual(sizes, [1])
    }

    func testRandomWithinNearTopGroup() {
        let active = account("a", five: 96, seven: 10, active: true)
        let top = account("top", five: 0, seven: 58, sevenResets: now + day)         // 40
        let close = account("close", five: 0, seven: 62, sevenResets: now + day)     // 36 >= 34
        let far = account("far", five: 0, seven: 8, sevenResets: now + 6 * day)      // 15
        var sizes: [Int] = []
        let first = decide(snapshot([active, top, close, far]), pick: { sizes.append($0); return 0 })
        let last = decide(snapshot([active, top, close, far]), pick: { $0 - 1 })
        XCTAssertEqual(sizes, [2])
        XCTAssertEqual(first, .switchTo(top.id, .threshold(tier: 1)))
        XCTAssertEqual(last, .switchTo(close.id, .threshold(tier: 1)))
    }

    func testZeroScoresMakeTheWholeTierTheGroup() {
        let active = account("a", five: 96, seven: 10, active: true)
        let fresh1 = account("f1", five: 0, seven: 0)
        let fresh2 = account("f2", five: 0, seven: 0)
        var sizes: [Int] = []
        _ = decide(snapshot([active, fresh1, fresh2]), pick: { sizes.append($0); return 0 })
        XCTAssertEqual(sizes, [2])
    }

    // MARK: Tiers 2 and 3

    func testTierTwoPicksLowestFiveHour() {
        let active = account("a", five: 96, seven: 10, active: true)
        let b = account("b", five: 93, seven: 50)
        let c = account("c", five: 92, seven: 60)
        let d = account("d", five: 10, seven: 99.5)   // tier 3, must lose to tier 2
        XCTAssertEqual(decide(snapshot([active, b, c, d])), .switchTo(c.id, .threshold(tier: 2)))
    }

    func testTierThreeIsTheLastResort() {
        let active = account("a", five: 96, seven: 10, active: true)
        let d = account("d", five: 10, seven: 99.5)
        let e = account("e", five: 50, seven: 99.2)
        let f = account("f", five: 5, seven: 99.2)
        XCTAssertEqual(decide(snapshot([active, d, e, f])), .switchTo(f.id, .threshold(tier: 3)))
    }

    // MARK: Lock

    func testLockRaisesOnlyTheLockedWindow() {
        let active = account("a", five: 10, seven: 99.5, sevenResets: now + day, active: true)
        let other = account("b", five: 0, seven: 10)
        let lock = AutoSwitchLock(accountId: active.id, fiveHour: nil,
                                  sevenDay: .init(resetsAt: now + day))
        XCTAssertEqual(decide(snapshot([active, other], lock: lock)), .stay(.belowThreshold))

        let hot = account("a", five: 95, seven: 99.5, sevenResets: now + day, active: true)
        let hotLock = AutoSwitchLock(accountId: hot.id, fiveHour: nil, sevenDay: .init(resetsAt: now + day))
        XCTAssertEqual(decide(snapshot([hot, other], lock: hotLock)), .switchTo(other.id, .threshold(tier: 1)))
    }

    func testMakeLockCoversOnlyWindowsAtThreshold() {
        let usage = UsageData(fiveHour: UsageLimit(utilization: 93, resetsAt: now + 3600),
                              sevenDay: UsageLimit(utilization: 99.4, resetsAt: now + day))
        let id = UUID()
        XCTAssertEqual(AutoSwitchPlanner.makeLock(for: id, usage: usage, thresholds: base),
                       AutoSwitchLock(accountId: id, fiveHour: nil, sevenDay: .init(resetsAt: now + day)))
        let calm = UsageData(fiveHour: UsageLimit(utilization: 10, resetsAt: nil),
                             sevenDay: UsageLimit(utilization: 10, resetsAt: nil))
        XCTAssertNil(AutoSwitchPlanner.makeLock(for: id, usage: calm, thresholds: base))
    }

    func testUpdatedLockClearsWhenUsedUp() {
        let active = account("a", five: 10, seven: 100, sevenResets: now + day, active: true)
        let lock = AutoSwitchLock(accountId: active.id, fiveHour: nil, sevenDay: .init(resetsAt: now + day))
        XCTAssertNil(AutoSwitchPlanner.updatedLock(lock, active: active))
    }

    func testUpdatedLockClearsOnReset() {
        let active = account("a", five: 10, seven: 2, sevenResets: now + 7 * day, active: true)
        let lock = AutoSwitchLock(accountId: active.id, fiveHour: nil, sevenDay: .init(resetsAt: now + day))
        XCTAssertNil(AutoSwitchPlanner.updatedLock(lock, active: active))
    }

    func testUpdatedLockKeepsWithinJitter() {
        let active = account("a", five: 10, seven: 99.6, sevenResets: now + day + 0.4, active: true)
        let lock = AutoSwitchLock(accountId: active.id, fiveHour: nil, sevenDay: .init(resetsAt: now + day))
        XCTAssertEqual(AutoSwitchPlanner.updatedLock(lock, active: active), lock)
    }

    func testUpdatedLockClearsWhenActiveChanges() {
        let other = account("b", five: 10, seven: 99.6, active: true)
        let lock = AutoSwitchLock(accountId: UUID(), fiveHour: nil, sevenDay: .init(resetsAt: now + day))
        XCTAssertNil(AutoSwitchPlanner.updatedLock(lock, active: other))
        XCTAssertNil(AutoSwitchPlanner.updatedLock(lock, active: nil))
    }

    // MARK: Priority tick

    func testPrioritySwitchesAfterResetEvenBelowThreshold() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 40, fiveResets: nil)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: now - 600)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .switchTo(ticked.id, .priorityReset))
    }

    func testPriorityFiresOnLaterResetsAt() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 2, seven: 40, fiveResets: now + 5 * 3600)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: now - 600)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .switchTo(ticked.id, .priorityReset))
    }

    func testPriorityWaitsBeforeReset() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 80, seven: 40, fiveResets: now + 600)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: now + 600)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .stay(.belowThreshold))
    }

    func testPriorityIgnoresSubSecondDrift() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 80, seven: 40, fiveResets: now + 600.4)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: now + 600)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .stay(.belowThreshold))
    }

    func testPriorityWithNilBaselineSwitchesNow() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 40, fiveResets: nil)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: nil)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .switchTo(ticked.id, .priorityReset))
    }

    func testPriorityWaitsWhileABaselineAheadHasNoWindow() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 40, fiveResets: nil)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: now + 3600)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .stay(.belowThreshold))
    }

    func testPriorityFiresOnceABaselineAheadHasPassed() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 40, fiveResets: nil)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: now - 1)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .switchTo(ticked.id, .priorityReset))
    }

    func testPriorityBaselineIsTheWindowResetOrFiveHoursAhead() {
        let started = UsageData(fiveHour: UsageLimit(utilization: 5, resetsAt: now + 600),
                                sevenDay: UsageLimit(utilization: 5, resetsAt: nil))
        let unstarted = UsageData(fiveHour: UsageLimit(utilization: 0, resetsAt: nil),
                                  sevenDay: UsageLimit(utilization: 5, resetsAt: nil))
        XCTAssertEqual(AutoSwitchPlanner.priorityBaseline(started, now: now), now + 600)
        XCTAssertEqual(AutoSwitchPlanner.priorityBaseline(unstarted, now: now), now + 5 * 3600)
    }

    func testPriorityBlockedBySevenDay99() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 99, fiveResets: nil)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: now - 600)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .stay(.belowThreshold))
    }

    func testPriorityIgnoredWhenNotReady() {
        let active = account("a", five: 10, seven: 10, active: true)
        let ticked = account("t", five: 0, seven: 40, availability: .loginExpired)
        let p = AutoSwitchPriority(accountId: ticked.id, baseline: now - 600)
        XCTAssertEqual(decide(snapshot([active, ticked], priority: p)), .stay(.belowThreshold))
    }
}
