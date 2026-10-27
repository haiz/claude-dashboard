import Foundation

/// The one process-wide clock behind the burn-animal display (never one per card).
///
/// It publishes the shared `AnimalDisplayModels.Phase`, sampled at 12 fps while running, and
/// runs only while `shouldRun()` is true (setting on and some account eligible). Every tick
/// re-asks `shouldRun`, so an account whose 600 s activity window lapses with no refresh
/// still stops the clock. `reevaluate()` starts or stops it on demand (refresh, setting).
///
/// Time comes from `now`, a monotonic source (`ProcessInfo.systemUptime` by default). The
/// activity tracker must be fed from the same source. Tests inject `now` and `scheduler`
/// and call the captured tick by hand, so nothing sleeps.
///
///     let clock = AnimalDisplayClock()
///     clock.shouldRun = { showBurnAnimals && anyAccountActive }
///     clock.reevaluate()          // starts the timer
///     clock.phase.frameIndex      // what a gauge draws
@MainActor
final class AnimalDisplayClock: ObservableObject {
    /// Calls `tick` every `interval` seconds until the returned closure is called.
    typealias Scheduler = (_ interval: TimeInterval, _ tick: @escaping @MainActor () -> Void) -> () -> Void

    static let monotonicNow: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }

    static let timerScheduler: Scheduler = { interval, tick in
        let timer = Timer(timeInterval: interval, repeats: true) { _ in
            MainActor.assumeIsolated { tick() }
        }
        RunLoop.main.add(timer, forMode: .common)
        return { timer.invalidate() }
    }

    @Published private(set) var phase: AnimalDisplayModels.Phase
    private(set) var isRunning = false
    var shouldRun: () -> Bool = { false }

    let now: () -> TimeInterval
    private let origin: TimeInterval
    private let scheduler: Scheduler
    private var cancel: (() -> Void)?

    init(now: @escaping () -> TimeInterval = AnimalDisplayClock.monotonicNow,
         scheduler: @escaping Scheduler = AnimalDisplayClock.timerScheduler) {
        self.now = now
        self.scheduler = scheduler
        self.origin = now()
        self.phase = .at(elapsedSeconds: 0)
    }

    /// Starts the timer when `shouldRun()` and it is stopped; stops it when not.
    func reevaluate() {
        if shouldRun() {
            guard !isRunning else { return }
            isRunning = true
            phase = .at(elapsedSeconds: now() - origin)
            cancel = scheduler(1 / AnimalDisplayModels.frameRate) { [weak self] in self?.tick() }
        } else {
            stop()
        }
    }

    private func tick() {
        phase = .at(elapsedSeconds: now() - origin)
        if !shouldRun() { stop() }
    }

    private func stop() {
        guard isRunning else { return }
        isRunning = false
        cancel?()
        cancel = nil
    }
}
