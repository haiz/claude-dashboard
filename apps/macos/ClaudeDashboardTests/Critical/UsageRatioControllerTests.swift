import XCTest
@testable import ClaudeDashboard

@MainActor
final class UsageRatioControllerTests: XCTestCase {
    private var clock = Date(timeIntervalSince1970: 1_800_000_000)
    private let a = UUID()

    private func rows(fivePerWindow: Double, sevenPerWindow: Double) -> UsageRatioController.LogFetch {
        let t = clock
        return { _, _ in
            let five = [UsageLogEntry(id: 0, accountId: self.a, window: .fiveHour, resetsAt: t, recordedAt: t.addingTimeInterval(-7200), utilization: 0, isLimited: false),
                        UsageLogEntry(id: 0, accountId: self.a, window: .fiveHour, resetsAt: t, recordedAt: t.addingTimeInterval(-60), utilization: fivePerWindow, isLimited: false)]
            let seven = [UsageLogEntry(id: 0, accountId: self.a, window: .sevenDay, resetsAt: t.addingTimeInterval(86_400), recordedAt: t.addingTimeInterval(-7200), utilization: 10, isLimited: false),
                         UsageLogEntry(id: 0, accountId: self.a, window: .sevenDay, resetsAt: t.addingTimeInterval(86_400), recordedAt: t.addingTimeInterval(-60), utilization: 10 + sevenPerWindow, isLimited: false)]
            return (five, seven)
        }
    }

    private func makeController(_ fetch: @escaping UsageRatioController.LogFetch,
                                store: UsageRatioStore = UsageRatioStore(dbPath: ":memory:")) -> UsageRatioController {
        UsageRatioController(store: store, fetchLogs: fetch, now: { [unowned self] in self.clock })
    }

    func testStartsAtTheDefault() {
        let c = makeController(rows(fivePerWindow: 100, sevenPerWindow: 16))
        XCTAssertEqual(c.current, UsageRatioValue(x: 16, updatedAt: nil))
        XCTAssertTrue(c.current.isDefault)
    }

    func testRecomputeStoresAndPublishes() async {
        let c = makeController(rows(fivePerWindow: 100, sevenPerWindow: 17))
        await c.recompute()
        XCTAssertEqual(c.current, UsageRatioValue(x: 17, updatedAt: clock))
        let history = await c.history(from: nil)
        XCTAssertEqual(history.map(\.x), [17])
    }

    func testNotEnoughDataKeepsTheLatestStoredValue() async {
        let store = UsageRatioStore(dbPath: ":memory:")
        await store.insert(UsageRatioPoint(date: clock.addingTimeInterval(-3600), x: 15.5, sumFiveHour: 300, sumSevenDay: 46.5))
        let c = makeController(rows(fivePerWindow: 50, sevenPerWindow: 8), store: store)
        await c.recompute()
        XCTAssertEqual(c.current, UsageRatioValue(x: 15.5, updatedAt: clock.addingTimeInterval(-3600)))
        let history = await c.history(from: nil)
        XCTAssertEqual(history.count, 1)
    }

    func testRecomputeDropsHistoryOlderThan90Days() async {
        let store = UsageRatioStore(dbPath: ":memory:")
        await store.insert(UsageRatioPoint(date: clock.addingTimeInterval(-91 * 86_400), x: 12, sumFiveHour: 100, sumSevenDay: 12))
        let c = makeController(rows(fivePerWindow: 100, sevenPerWindow: 16), store: store)
        await c.recompute()
        let history = await c.history(from: nil)
        XCTAssertEqual(history.map(\.x), [16])
    }

    func testFetchWindowsAreLookbackAndLookbackPlusAWeek() async {
        var asked: (Date, Date)?
        let c = makeController({ five, seven in asked = (five, seven); return ([], []) })
        await c.recompute()
        XCTAssertEqual(asked?.0, clock.addingTimeInterval(-3 * 86_400))
        XCTAssertEqual(asked?.1, clock.addingTimeInterval(-10 * 86_400))
    }
}
