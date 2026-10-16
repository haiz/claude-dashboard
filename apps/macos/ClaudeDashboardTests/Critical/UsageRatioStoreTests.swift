import XCTest
@testable import ClaudeDashboard

final class UsageRatioStoreTests: XCTestCase {
    private var dbPath: String!
    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    override func setUp() {
        super.setUp()
        dbPath = NSTemporaryDirectory() + "ratio_logs_\(UUID().uuidString).db"
    }

    override func tearDown() {
        try? FileManager.default.removeItem(atPath: dbPath)
        super.tearDown()
    }

    private func point(_ hours: Double, _ x: Double) -> UsageRatioPoint {
        UsageRatioPoint(date: t0.addingTimeInterval(hours * 3600), x: x, sumFiveHour: 200, sumSevenDay: x * 2)
    }

    func testEmptyStoreHasNoLatest() async {
        let latest = await UsageRatioStore(dbPath: dbPath).latest()
        XCTAssertNil(latest)
    }

    func testLatestAndSeriesRoundTripEveryField() async {
        let store = UsageRatioStore(dbPath: dbPath)
        await store.insert(point(0, 16.1))
        await store.insert(point(1, 16.4))
        let latest = await store.latest()
        let all = await store.series(from: nil, to: nil)
        XCTAssertEqual(latest, point(1, 16.4))
        XCTAssertEqual(all, [point(0, 16.1), point(1, 16.4)])
    }

    func testSeriesRange() async {
        let store = UsageRatioStore(dbPath: dbPath)
        for h in 0..<5 { await store.insert(point(Double(h), 16)) }
        let mid = await store.series(from: t0.addingTimeInterval(3600), to: t0.addingTimeInterval(3 * 3600))
        XCTAssertEqual(mid.map(\.date), [1, 2, 3].map { t0.addingTimeInterval($0 * 3600) })
    }

    func testDeleteOlderThan() async {
        let store = UsageRatioStore(dbPath: dbPath)
        for h in 0..<3 { await store.insert(point(Double(h), 16)) }
        await store.deleteOlderThan(t0.addingTimeInterval(3600))
        let left = await store.series(from: nil, to: nil)
        XCTAssertEqual(left.map(\.date), [t0.addingTimeInterval(3600), t0.addingTimeInterval(7200)])
    }

    func testPersistsAcrossInstances() async {
        await UsageRatioStore(dbPath: dbPath).insert(point(0, 15.9))
        let latest = await UsageRatioStore(dbPath: dbPath).latest()
        XCTAssertEqual(latest?.x, 15.9)
    }

    func testInMemoryStoreWorks() async {
        let store = UsageRatioStore(dbPath: ":memory:")
        await store.insert(point(0, 16))
        let latest = await store.latest()
        XCTAssertEqual(latest?.x, 16)
    }
}
