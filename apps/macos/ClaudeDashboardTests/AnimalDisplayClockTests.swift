import XCTest
import Combine
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
        clock.gaugeAppeared()           // one 5h gauge center on screen
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

    func testCycleRestartsWhenAGaugeMountsWithNoneOnScreen() {
        eligible = true
        time = 100
        let clock = makeClock()         // popover opened at t=100
        time = 107
        tick?()
        XCTAssertEqual(clock.phase.content, .animal)
        clock.gaugeDisappeared()        // popover closed
        time = 237                      // reopened where the old origin shows the animal
        clock.gaugeAppeared()
        XCTAssertEqual(clock.phase.phase, 0, accuracy: 1e-9, "a fresh open starts on the percentage")
        XCTAssertEqual(clock.phase.content, .percentage)
        time = 243
        tick?()
        XCTAssertEqual(clock.phase.phase, 6, accuracy: 1e-9)
        XCTAssertEqual(clock.phase.content, .animal, "animal 6 s after opening")
        time = 244
        clock.gaugeAppeared()           // a second surface opens while one is shown: same cycle
        tick?()
        XCTAssertEqual(clock.phase.phase, 7, accuracy: 1e-9)
    }

    func testRunsOnlyWhileAGaugeIsMounted() {
        let clock = makeClock()
        clock.gaugeDisappeared()        // nothing on screen
        eligible = true
        clock.reevaluate()
        XCTAssertFalse(clock.isRunning, "eligible but no gauge mounted")
        XCTAssertEqual(schedules, 0)
        clock.gaugeAppeared()
        XCTAssertTrue(clock.isRunning, "a mounted gauge starts it")
        clock.gaugeAppeared()           // a second gauge: still one timer
        clock.gaugeDisappeared()
        XCTAssertTrue(clock.isRunning, "one gauge still mounted")
        XCTAssertEqual(schedules, 1)
        clock.gaugeDisappeared()
        XCTAssertFalse(clock.isRunning, "the last gauge left")
        XCTAssertEqual(cancels, 1)
        clock.gaugeDisappeared()        // unbalanced: never negative
        XCTAssertEqual(clock.mountedGauges, 0)
    }

    func testPercentageOnlyTicksPublishNothing_animalTicksPublishEachFrame() {
        let clock = makeClock()
        eligible = true
        time = 101                      // 1 s into the cycle: percentage only
        clock.reevaluate()
        var publishes = 0
        let sub = clock.objectWillChange.sink { publishes += 1 }
        defer { sub.cancel() }
        var t = 101.0
        while t < 105.4 {               // the rest of the percentage-only part, at 12 fps
            t += 1 / 12; time = t
            tick?()
        }
        XCTAssertEqual(publishes, 0, "no redraw while only the percentage shows")
        time = 107; tick?()             // animal frame 12
        time = 107.1; tick?()           // frame 13
        XCTAssertEqual(publishes, 2)
        XCTAssertEqual(clock.phase.frameIndex, 13)
        eligible = false                // expiry is still re-checked on a quiet tick
        time = 111; tick?()
        XCTAssertFalse(clock.isRunning)
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
