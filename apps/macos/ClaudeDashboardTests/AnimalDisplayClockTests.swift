import XCTest
@testable import ClaudeDashboard

@MainActor
final class AnimalDisplayClockTests: XCTestCase {
    private var time: TimeInterval = 100
    private var tick: (@MainActor () -> Void)?
    private var schedules = 0
    private var cancels = 0
    private var interval: TimeInterval = 0
    private var eligible = false

    private func makeClock() -> AnimalDisplayClock {
        let clock = AnimalDisplayClock(
            now: { self.time },
            scheduler: { interval, tick in
                self.schedules += 1; self.interval = interval; self.tick = tick
                return { self.cancels += 1; self.tick = nil }
            })
        clock.shouldRun = { self.eligible }
        return clock
    }

    func testDoesNotStartWhileNothingEligible() {
        let clock = makeClock()
        clock.reevaluate()
        XCTAssertFalse(clock.isRunning)
        XCTAssertEqual(schedules, 0)
    }

    func testStartsOnce_at12fps_andPhaseFollowsInjectedTime() {
        let clock = makeClock()
        eligible = true
        time = 107                      // 7 s after creation at t=100
        clock.reevaluate(); clock.reevaluate()
        XCTAssertEqual(schedules, 1, "one timer however often it is re-evaluated")
        XCTAssertEqual(interval, 1 / 12, accuracy: 1e-9)
        XCTAssertEqual(clock.phase.phase, 7, accuracy: 1e-9)
        time = 115.5
        tick?()
        XCTAssertEqual(clock.phase.phase, 5.5, accuracy: 1e-9)
        XCTAssertEqual(clock.phase.content, .percentageToAnimal)
    }

    func testStopsOnTickWhenEligibilityLapsesAndRestarts() {
        let clock = makeClock()
        eligible = true
        clock.reevaluate()
        eligible = false                // activity window ran out, no refresh happened
        tick?()
        XCTAssertFalse(clock.isRunning)
        XCTAssertEqual(cancels, 1)
        eligible = true
        clock.reevaluate()
        XCTAssertEqual(schedules, 2)
        XCTAssertTrue(clock.isRunning)
    }

    func testReevaluateStopsWhenSettingTurnsOff() {
        let clock = makeClock()
        eligible = true
        clock.reevaluate()
        eligible = false
        clock.reevaluate()
        XCTAssertFalse(clock.isRunning)
        XCTAssertEqual(cancels, 1)
    }
}
