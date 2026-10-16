import Foundation

/// The X the app uses now. `updatedAt == nil` means no measurement yet: `x` is the default.
struct UsageRatioValue: Equatable {
    let x: Double
    let updatedAt: Date?
    var isDefault: Bool { updatedAt == nil }
}

/// Recomputes X at `start()` and every hour, stores each measurement and publishes the
/// latest. A run without enough data stores nothing and keeps the previous value.
///
///     let c = UsageRatioController.live(logStore: store, isRunningTests: AppDefaults.isRunningTests())
///     c.start()
///     c.current.x        // 16.3
@MainActor
final class UsageRatioController: ObservableObject {
    typealias LogFetch = (_ fiveHourFrom: Date, _ sevenDayFrom: Date) async
        -> (fiveHour: [UsageLogEntry], sevenDay: [UsageLogEntry])

    static let interval: TimeInterval = 3600
    static let retention: TimeInterval = 90 * 86_400

    @Published private(set) var current = UsageRatioValue(x: UsageRatioEstimator.defaultX, updatedAt: nil)
    let store: UsageRatioStore
    private let fetchLogs: LogFetch
    private let now: () -> Date
    private var task: Task<Void, Never>?

    init(store: UsageRatioStore, fetchLogs: @escaping LogFetch, now: @escaping () -> Date = Date.init) {
        self.store = store
        self.fetchLogs = fetchLogs
        self.now = now
    }

    /// Production wiring; under XCTest the history lives in memory, never in `ratio_logs.db`.
    static func live(logStore: UsageLogStore, isRunningTests: Bool) -> UsageRatioController {
        UsageRatioController(
            store: UsageRatioStore(dbPath: isRunningTests ? ":memory:" : nil),
            fetchLogs: { fiveFrom, sevenFrom in
                let five = await logStore.allLogs(window: .fiveHour, from: fiveFrom, to: nil)
                let seven = await logStore.allLogs(window: .sevenDay, from: sevenFrom, to: nil)
                return (five, seven)
            })
    }

    func start() {
        task?.cancel()
        task = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                await self.recompute()
                try? await Task.sleep(nanoseconds: UInt64(Self.interval * 1_000_000_000))
            }
        }
    }

    func recompute() async {
        let t = now()
        let lookbackStart = t.addingTimeInterval(-UsageRatioEstimator.lookback)
        let logs = await fetchLogs(lookbackStart, lookbackStart.addingTimeInterval(-UsageRatioEstimator.sevenDayLookback))
        if let e = UsageRatioEstimator.estimate(fiveHour: logs.fiveHour, sevenDay: logs.sevenDay) {
            await store.insert(UsageRatioPoint(date: t, x: e.x, sumFiveHour: e.sumFiveHour, sumSevenDay: e.sumSevenDay))
        }
        await store.deleteOlderThan(t.addingTimeInterval(-Self.retention))
        if let latest = await store.latest() {
            current = UsageRatioValue(x: latest.x, updatedAt: latest.date)
        }
    }

    func history(from: Date?) async -> [UsageRatioPoint] {
        await store.series(from: from, to: nil)
    }
}
