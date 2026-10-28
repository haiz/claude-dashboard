import Foundation

/// The one process-wide clock behind the burn-animal display (never one per card).
///
/// It publishes the shared `AnimalDisplayModels.Phase`, counted from when the first gauge
/// center mounted (so opening the popover or a window starts on the percentage), sampled at
/// 12 fps while running, and
/// runs only while some gauge center is mounted (`gaugeAppeared` / `gaugeDisappeared`, so a
/// closed window and popover cost nothing) and `shouldRun()` is true (setting on and some
/// account eligible). Every tick re-asks `shouldRun`, so an account whose 600 s activity
/// window lapses with no refresh still stops the clock. `reevaluate()` starts or stops it on
/// demand (refresh, setting). A tick publishes only when what a gauge draws changes
/// (`Phase.rendersSame`), so the 5.5 s percentage-only part of the cycle redraws nothing.
///
/// Time comes from `now`, a monotonic source (`ProcessInfo.systemUptime` by default). The
/// activity tracker must be fed from the same source. Tests inject `now` and `scheduler`
/// and call the captured tick by hand, so nothing sleeps.
///
///     let clock = AnimalDisplayClock()
///     clock.shouldRun = { showBurnAnimals && anyAccountActive }
///     clock.gaugeAppeared()       // a 5h gauge center mounted: starts the timer
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
    /// Gauge centers on screen that observe this clock (`ObservingGaugeCenter`).
    private(set) var mountedGauges = 0
    var shouldRun: () -> Bool = { false }

    let now: () -> TimeInterval
    /// Start of the 10 s cycle: reset when the first gauge mounts (a window or the popover
    /// opens with none on screen), so every open starts on the percentage.
    private var origin: TimeInterval
    private let scheduler: Scheduler
    private var cancel: (() -> Void)?

    init(now: @escaping () -> TimeInterval = AnimalDisplayClock.monotonicNow,
         scheduler: @escaping Scheduler = AnimalDisplayClock.timerScheduler) {
        self.now = now
        self.scheduler = scheduler
        self.origin = now()
        self.phase = .at(elapsedSeconds: 0)
    }

    /// A gauge center that observes the clock appeared; starts it when something is eligible.
    /// The first one on screen restarts the cycle; later ones join it.
    func gaugeAppeared() {
        if mountedGauges == 0 {
            origin = now()
            phase = .at(elapsedSeconds: 0)
        }
        mountedGauges += 1
        reevaluate()
    }

    /// A gauge center disappeared; the last one stops the clock.
    func gaugeDisappeared() {
        mountedGauges = max(0, mountedGauges - 1)
        reevaluate()
    }

    /// Starts the timer when a gauge is mounted and `shouldRun()`, and it is stopped; stops
    /// it when not.
    func reevaluate() {
        if mountedGauges > 0 && shouldRun() {
            guard !isRunning else { return }
            isRunning = true
            phase = .at(elapsedSeconds: now() - origin)
            cancel = scheduler(1 / AnimalDisplayModels.frameRate) { [weak self] in self?.tick() }
        } else {
            stop()
        }
    }

    private func tick() {
        let next = AnimalDisplayModels.Phase.at(elapsedSeconds: now() - origin)
        if !next.rendersSame(as: phase) { phase = next }
        // Re-checked every tick, published or not: the 600 s expiry needs no refresh.
        if mountedGauges == 0 || !shouldRun() { stop() }
    }

    private func stop() {
        guard isRunning else { return }
        isRunning = false
        cancel?()
        cancel = nil
    }
}
