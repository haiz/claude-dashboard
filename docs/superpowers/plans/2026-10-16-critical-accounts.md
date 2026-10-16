# Critical Accounts and Ratio X Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure X (7d% per full 5h window) hourly and chart it on the Dashboard, mark accounts whose 7d quota will go unused as critical / high critical, and let auto switch pull Claude Code onto them by the agreed priority.

**Architecture:** Pure cores (`UsageRatioEstimator`, `CriticalPlanner`, `AutoSwitchPlanner`) hold every rule; one SQLite actor (`UsageRatioStore`) keeps X history; one `@MainActor` controller (`UsageRatioController`) runs the hourly job; `CriticalSettings` persists the user's paces and quiet hours. `DashboardViewModel` (962 lines) only owns and forwards them, with its glue in a new `DashboardViewModel+Critical.swift`.

**Tech Stack:** Swift 5, SwiftUI, Swift Charts, SQLite3, XCTest, XcodeGen. macOS 13 deployment target.

**Spec:** `docs/superpowers/specs/2026-10-16-critical-accounts-design.md`

## Global Constraints

- Scope: `apps/macos/ClaudeDashboard` only. Linux, Windows, helper CLI, `Account` and `contract/` untouched.
- No external dependencies; Swift Charts is allowed (already used by `OverviewChartView`).
- App code reads `AppDefaults.shared`, never `UserDefaults.standard`. Settings classes take an injected `UserDefaults`.
- Under XCTest the ratio store opens `":memory:"` (`AppDefaults.isRunningTests()`), never the real `ratio_logs.db`.
- X default **16**; lookback **3 days**; per-window minimum Δ5h **10**; minimum total Δ5h **100**; `resetsAt` jitter tolerance **60 s**; 5h/7d sample tolerance **5 s**; history retention **90 days**; recompute at launch and every **3600 s**.
- Critical defaults: pace **50**, high pace **80** (range 10...100, step 5, high >= pace), quiet hours **0 to 7**, time zone **Asia/Ho_Chi_Minh**; start == end means no quiet hours.
- A critical target's 5h is free when `resetsAt == nil` or `5h <= 2`.
- Priority order: high critical > home (bolt tick) > critical > ordinary threshold tiers.
- All UI and Help text English. Files under 1000 lines; `DashboardViewModel.swift` grows by at most ~15 lines.
- After adding any file run `cd apps/macos && xcodegen generate`.
- Never add Claude/Anthropic attribution to commits.

## Review Focus

1. **An exhausted or nearly exhausted week near its reset** (7d at 100%, or 99.5% one hour before reset) must not show a critical badge: a reasonable person reads "critical" as "quota left to burn". Pinned by `CriticalPlannerTests.testExhaustedWeekIsNotCritical`.
2. **`resetsAt` jitter in usage logs** (the same 5h window logged as `...799` and `...800`) must not split a window and drop it under the 10% minimum. Pinned by `UsageRatioEstimatorTests.testJitteredResetsAtIsOneWindow`.
3. **Settings stored by an older build or edited by hand** (high pace below pace, unknown time zone id, hour 30) must load as valid values, not crash or produce nonsense projections. Pinned by `CriticalSettingsTests.testInvalidStoredValuesFallBack`.
4. **A machine whose system time zone differs from the quiet-hours zone** must still exclude 00:00 to 07:00 Vietnam time, not local time. Pinned by `CriticalPlannerTests.testQuietHoursUseTheirOwnTimeZone`.
5. **An account the user just switched to by hand while it is critical** must not be pulled away by another critical account on the next refresh (no ping-pong between two critical accounts). Pinned by `AutoSwitchPlannerTests.testCriticalDoesNotPullFromAnotherCritical`.

---

## File Structure

| File | Responsibility |
|---|---|
| Create `ClaudeDashboard/Services/Critical/UsageRatioEstimator.swift` | Pure X estimate from log rows |
| Create `ClaudeDashboard/Services/Critical/UsageRatioStore.swift` | `ratio_logs.db` actor: insert, latest, series, retention |
| Create `ClaudeDashboard/Services/Critical/UsageRatioController.swift` | Hourly job, `current` value, history read |
| Create `ClaudeDashboard/Services/Critical/CriticalPlanner.swift` | Quiet-hours math, assessment, badge text |
| Create `ClaudeDashboard/Models/CriticalSettings.swift` | Persisted paces, quiet hours, time zone |
| Create `ClaudeDashboard/ViewModels/DashboardViewModel+Critical.swift` | Assessment and badge per account state |
| Create `ClaudeDashboard/Views/UsageRatioChart.swift` | X button and its popover chart |
| Create `ClaudeDashboard/Views/Components/InfoTip.swift` | Info icon: hover tooltip, click popover |
| Create `ClaudeDashboard/Views/Settings/SettingsTips.swift` | Text of every setting's tip |
| Create `ClaudeDashboard/Views/Settings/CriticalSettingsSection.swift` | Settings section for critical |
| Modify `ClaudeDashboard/Services/AutoSwitch/AutoSwitchPlanner.swift` | `critical` on accounts, new reason, rules 1 to 3 |
| Modify `ClaudeDashboard/Services/AutoSwitch/AutoSwitchController.swift` | Footer and log text for the new reason |
| Modify `ClaudeDashboard/ViewModels/DashboardViewModel.swift` | Own controller + settings, `helpFocus` |
| Modify `ClaudeDashboard/ViewModels/DashboardViewModel+AutoSwitch.swift` | Pass `critical` into `AutoSwitchAccount` |
| Modify `ClaudeDashboard/Views/AccountCard.swift` | Flame badge |
| Modify `ClaudeDashboard/Views/DashboardPane.swift`, `MenuBarPopover.swift` | X button, badge argument |
| Modify `ClaudeDashboard/Views/Settings/GeneralSettingsPane.swift` | Tips on every setting, new section |
| Modify `ClaudeDashboard/Views/HelpView.swift`, `MainWindow.swift` | New topic, updated topics, focus scroll |
| Tests under `ClaudeDashboardTests/Critical/` | One file per unit |

---

# Part A: ratio X

### Task 1: UsageRatioEstimator

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/Critical/UsageRatioEstimator.swift`
- Test: `apps/macos/ClaudeDashboardTests/Critical/UsageRatioEstimatorTests.swift`

**Interfaces:**
- Consumes: `UsageLogEntry` (`Models/UsageLogModels.swift`: `accountId`, `window`, `resetsAt`, `recordedAt`, `utilization`).
- Produces: `struct UsageRatioEstimate: Equatable { let x, sumFiveHour, sumSevenDay: Double }`; `enum UsageRatioEstimator` with `static let lookback: TimeInterval`, `defaultX = 16.0`, `static func estimate(fiveHour: [UsageLogEntry], sevenDay: [UsageLogEntry]) -> UsageRatioEstimate?`.

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

final class UsageRatioEstimatorTests: XCTestCase {
    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)
    private let a = UUID()
    private let b = UUID()

    private func row(_ account: UUID, _ window: UsageWindow, at minutes: Double,
                     resets: TimeInterval, _ u: Double) -> UsageLogEntry {
        UsageLogEntry(id: 0, accountId: account, window: window,
                      resetsAt: t0.addingTimeInterval(resets),
                      recordedAt: t0.addingTimeInterval(minutes * 60),
                      utilization: u, isLimited: false)
    }

    /// One account, one 5h window 0 -> 100 while 7d goes 10 -> 26: X = 16.
    func testSingleWindow() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0),
                    row(a, .fiveHour, at: 290, resets: 18_000, 100)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10),
                     row(a, .sevenDay, at: 290, resets: 500_000, 26)]
        XCTAssertEqual(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven),
                       UsageRatioEstimate(x: 16, sumFiveHour: 100, sumSevenDay: 16))
    }

    func testTwoAccountsArePooled() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0), row(a, .fiveHour, at: 100, resets: 18_000, 60),
                    row(b, .fiveHour, at: 0, resets: 19_000, 20), row(b, .fiveHour, at: 100, resets: 19_000, 60)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10), row(a, .sevenDay, at: 100, resets: 500_000, 20),
                     row(b, .sevenDay, at: 0, resets: 600_000, 40), row(b, .sevenDay, at: 100, resets: 600_000, 46)]
        let e = UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven)
        XCTAssertEqual(e?.sumFiveHour, 100)
        XCTAssertEqual(e?.sumSevenDay, 16)
        XCTAssertEqual(e?.x ?? 0, 16, accuracy: 1e-9)
    }

    func testJitteredResetsAtIsOneWindow() {
        let five = [row(a, .fiveHour, at: 0, resets: 17_999, 0),
                    row(a, .fiveHour, at: 100, resets: 18_000, 50),
                    row(a, .fiveHour, at: 200, resets: 17_999, 100)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10),
                     row(a, .sevenDay, at: 200, resets: 500_001, 26)]
        XCTAssertEqual(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven)?.sumFiveHour, 100)
    }

    func testWindowWithWeeklyResetInsideIsSkipped() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0), row(a, .fiveHour, at: 290, resets: 18_000, 100)]
        let seven = [row(a, .sevenDay, at: 0, resets: 1_000, 90), row(a, .sevenDay, at: 290, resets: 600_000, 5)]
        XCTAssertNil(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven))
    }

    func testSmallWindowsAreSkipped() {
        var five: [UsageLogEntry] = []
        for i in 0..<20 {   // 20 windows of +9 each: 180 total, but every one is under 10
            let resets = Double(i) * 18_000 + 18_000
            five += [row(a, .fiveHour, at: Double(i) * 300, resets: resets, 0),
                     row(a, .fiveHour, at: Double(i) * 300 + 100, resets: resets, 9)]
        }
        let seven = [row(a, .sevenDay, at: 0, resets: 900_000, 10)]
        XCTAssertNil(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven))
    }

    func testNotEnoughDataIsNil() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0), row(a, .fiveHour, at: 100, resets: 18_000, 99)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10), row(a, .sevenDay, at: 100, resets: 500_000, 26)]
        XCTAssertNil(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven))
    }

    /// Compression keeps only the first and last row of a run: the 7d reading at a time is the
    /// last row at or before it, even hours earlier. A 7d row 3 s after the 5h row still counts.
    func testSparseSevenDayRowsAndSampleTolerance() {
        let five = [row(a, .fiveHour, at: 600, resets: 36_000, 0), row(a, .fiveHour, at: 890, resets: 36_000, 100)]
        let seven = [row(a, .sevenDay, at: 0, resets: 500_000, 10),
                     row(a, .sevenDay, at: 890 + 0.05, resets: 500_000, 26)]
        XCTAssertEqual(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: seven)?.x ?? 0, 16, accuracy: 1e-9)
    }

    func testMissingSevenDayIsSkipped() {
        let five = [row(a, .fiveHour, at: 0, resets: 18_000, 0), row(a, .fiveHour, at: 290, resets: 18_000, 100)]
        XCTAssertNil(UsageRatioEstimator.estimate(fiveHour: five, sevenDay: []))
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageRatioEstimatorTests 2>&1 | tail -20
```
Expected: build failure, `cannot find 'UsageRatioEstimator' in scope`.

- [ ] **Step 3: Implement**

```swift
import Foundation

/// X: the share of the 7d quota (in %) that one full 5h window costs, for all accounts
/// together. Spec: `docs/superpowers/specs/2026-10-16-critical-accounts-design.md`, "Estimate".
///
///     let e = UsageRatioEstimator.estimate(fiveHour: fiveRows, sevenDay: sevenRows)
///     e?.x   // 16.3, or nil when the logs hold under 100 points of 5h use
struct UsageRatioEstimate: Equatable {
    let x: Double
    let sumFiveHour: Double
    let sumSevenDay: Double
}

enum UsageRatioEstimator {
    static let lookback: TimeInterval = 3 * 86_400
    /// How far back 7d rows are read beyond `lookback`: compression can leave the last
    /// row of an unchanged 7d value days before the 5h window that needs it.
    static let sevenDayLookback: TimeInterval = 7 * 86_400
    static let minWindowDelta = 10.0
    static let minTotalFiveHour = 100.0
    static let defaultX = 16.0
    /// `resetsAt` drifts by a second or more between fetches; a real reset moves it by hours.
    static let resetTolerance: TimeInterval = 60
    /// The 5h and 7d rows of one refresh are written separately and can land a second apart.
    static let sampleTolerance: TimeInterval = 5

    static func estimate(fiveHour: [UsageLogEntry], sevenDay: [UsageLogEntry]) -> UsageRatioEstimate? {
        let weekly = Dictionary(grouping: sevenDay, by: \.accountId)
            .mapValues { $0.sorted { $0.recordedAt < $1.recordedAt } }
        var sumFive = 0.0
        var sumSeven = 0.0
        for window in windows(fiveHour) {
            guard let first = window.first, let last = window.last,
                  last.utilization - first.utilization >= minWindowDelta,
                  let series = weekly[first.accountId],
                  let w0 = reading(series, at: first.recordedAt),
                  let w1 = reading(series, at: last.recordedAt),
                  abs(w1.resetsAt.timeIntervalSince(w0.resetsAt)) <= resetTolerance else { continue }
            sumFive += last.utilization - first.utilization
            sumSeven += w1.utilization - w0.utilization
        }
        guard sumFive >= minTotalFiveHour else { return nil }
        return UsageRatioEstimate(x: 100 * sumSeven / sumFive, sumFiveHour: sumFive, sumSevenDay: sumSeven)
    }

    /// Per account, in time order; a `resetsAt` more than `resetTolerance` away from the
    /// current window's starts the next window.
    private static func windows(_ rows: [UsageLogEntry]) -> [[UsageLogEntry]] {
        var result: [[UsageLogEntry]] = []
        for (_, accountRows) in Dictionary(grouping: rows, by: \.accountId) {
            var current: [UsageLogEntry] = []
            for row in accountRows.sorted(by: { $0.recordedAt < $1.recordedAt }) {
                if let anchor = current.first,
                   abs(row.resetsAt.timeIntervalSince(anchor.resetsAt)) > resetTolerance {
                    result.append(current)
                    current = []
                }
                current.append(row)
            }
            if !current.isEmpty { result.append(current) }
        }
        return result
    }

    private static func reading(_ series: [UsageLogEntry], at time: Date) -> UsageLogEntry? {
        series.last { $0.recordedAt <= time.addingTimeInterval(sampleTolerance) }
    }
}
```

- [ ] **Step 4: Run to verify they pass** (same command). Expected: `Test Suite 'UsageRatioEstimatorTests' passed`, 8 tests.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/Critical/UsageRatioEstimator.swift apps/macos/ClaudeDashboardTests/Critical/UsageRatioEstimatorTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): estimate the 5h-to-7d ratio X from usage logs"
```

### Task 2: UsageRatioStore

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/Critical/UsageRatioStore.swift`
- Test: `apps/macos/ClaudeDashboardTests/Critical/UsageRatioStoreTests.swift`

**Interfaces:**
- Consumes: `PingLogStore.defaultDirectory()`.
- Produces: `struct UsageRatioPoint: Equatable { let date: Date; let x, sumFiveHour, sumSevenDay: Double }`; `actor UsageRatioStore { init(dbPath: String? = nil); func insert(_: UsageRatioPoint); func latest() -> UsageRatioPoint?; func series(from: Date?, to: Date?) -> [UsageRatioPoint]` (oldest first)`; func deleteOlderThan(_: Date); static func defaultDBPath() -> String }`.

- [ ] **Step 1: Write the failing tests**

```swift
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
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageRatioStoreTests 2>&1 | tail -20
```
Expected: `cannot find 'UsageRatioStore' in scope`.

- [ ] **Step 3: Implement**

```swift
import Foundation
import SQLite3

/// One hourly measurement of X.
struct UsageRatioPoint: Equatable {
    let date: Date
    let x: Double
    let sumFiveHour: Double
    let sumSevenDay: Double
}

/// History of X in `ratio_logs.db`. Dates are stored in whole seconds.
///
///     let store = UsageRatioStore()            // tests: UsageRatioStore(dbPath: ":memory:")
///     await store.insert(UsageRatioPoint(date: now, x: 16.3, sumFiveHour: 2145, sumSevenDay: 349))
///     await store.latest()?.x                  // 16.3
actor UsageRatioStore {
    private var db: OpaquePointer?

    init(dbPath: String? = nil) {
        openDatabase(at: dbPath ?? UsageRatioStore.defaultDBPath())
        sqlite3_exec(db, """
            CREATE TABLE IF NOT EXISTS ratio_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                t INTEGER NOT NULL, x REAL NOT NULL, d5 REAL NOT NULL, d7 REAL NOT NULL)
            """, nil, nil, nil)
        sqlite3_exec(db, "CREATE INDEX IF NOT EXISTS idx_ratio_t ON ratio_logs(t)", nil, nil, nil)
    }

    deinit {
        if let db { sqlite3_close(db) }
    }

    func insert(_ point: UsageRatioPoint) {
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, "INSERT INTO ratio_logs (t, x, d5, d7) VALUES (?, ?, ?, ?)",
                                 -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_int64(stmt, 1, Int64(point.date.timeIntervalSince1970))
        sqlite3_bind_double(stmt, 2, point.x)
        sqlite3_bind_double(stmt, 3, point.sumFiveHour)
        sqlite3_bind_double(stmt, 4, point.sumSevenDay)
        sqlite3_step(stmt)
    }

    func latest() -> UsageRatioPoint? {
        query("SELECT t, x, d5, d7 FROM ratio_logs ORDER BY t DESC, id DESC LIMIT 1", binds: []).first
    }

    /// Oldest first; nil bounds are open.
    func series(from: Date?, to: Date?) -> [UsageRatioPoint] {
        var sql = "SELECT t, x, d5, d7 FROM ratio_logs WHERE 1 = 1"
        var binds: [Int64] = []
        if let from { sql += " AND t >= ?"; binds.append(Int64(from.timeIntervalSince1970)) }
        if let to { sql += " AND t <= ?"; binds.append(Int64(to.timeIntervalSince1970)) }
        return query(sql + " ORDER BY t ASC, id ASC", binds: binds)
    }

    func deleteOlderThan(_ date: Date) {
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, "DELETE FROM ratio_logs WHERE t < ?", -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_int64(stmt, 1, Int64(date.timeIntervalSince1970))
        sqlite3_step(stmt)
    }

    private func query(_ sql: String, binds: [Int64]) -> [UsageRatioPoint] {
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }
        for (i, value) in binds.enumerated() { sqlite3_bind_int64(stmt, Int32(i + 1), value) }
        var rows: [UsageRatioPoint] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            rows.append(UsageRatioPoint(
                date: Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 0))),
                x: sqlite3_column_double(stmt, 1),
                sumFiveHour: sqlite3_column_double(stmt, 2),
                sumSevenDay: sqlite3_column_double(stmt, 3)))
        }
        return rows
    }

    private func openDatabase(at path: String) {
        if path != ":memory:" {
            let dir = (path as NSString).deletingLastPathComponent
            try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        }
        guard sqlite3_open(path, &db) == SQLITE_OK else {
            print("[UsageRatioStore] Failed to open database at \(path)")
            return
        }
    }

    static func defaultDBPath() -> String {
        PingLogStore.defaultDirectory().appendingPathComponent("ratio_logs.db").path
    }
}
```

- [ ] **Step 4: Run to verify they pass** (same command). Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/Critical/UsageRatioStore.swift apps/macos/ClaudeDashboardTests/Critical/UsageRatioStoreTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): keep the history of X in ratio_logs.db"
```

### Task 3: UsageRatioController

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/Critical/UsageRatioController.swift`
- Test: `apps/macos/ClaudeDashboardTests/Critical/UsageRatioControllerTests.swift`

**Interfaces:**
- Consumes: Task 1 `UsageRatioEstimator`, Task 2 `UsageRatioStore`, `UsageRatioPoint`; `UsageLogStore.allLogs(window:from:to:)`.
- Produces: `struct UsageRatioValue: Equatable { let x: Double; let updatedAt: Date?; var isDefault: Bool }`; `@MainActor final class UsageRatioController: ObservableObject { @Published private(set) var current: UsageRatioValue; init(store:fetchLogs:now:); static func live(logStore: UsageLogStore, isRunningTests: Bool) -> UsageRatioController; func start(); func recompute() async; func history(from: Date?) async -> [UsageRatioPoint] }`; `typealias UsageRatioController.LogFetch = (_ fiveHourFrom: Date, _ sevenDayFrom: Date) async -> (fiveHour: [UsageLogEntry], sevenDay: [UsageLogEntry])`.

- [ ] **Step 1: Write the failing tests**

```swift
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
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageRatioControllerTests 2>&1 | tail -20
```
Expected: `cannot find 'UsageRatioController' in scope`.

- [ ] **Step 3: Implement**

```swift
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
                await self?.recompute()
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
```

- [ ] **Step 4: Run to verify they pass** (same command). Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/Critical/UsageRatioController.swift apps/macos/ClaudeDashboardTests/Critical/UsageRatioControllerTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): recompute X hourly and publish the latest value"
```

### Task 4: X on the Dashboard with its chart

**Files:**
- Create: `apps/macos/ClaudeDashboard/Views/UsageRatioChart.swift`
- Modify: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift` (property, init param, init body)
- Modify: `apps/macos/ClaudeDashboard/Views/DashboardPane.swift:13-23` (PaneHeader trailing)
- Test: `apps/macos/ClaudeDashboardTests/Critical/UsageRatioChartTests.swift`

**Interfaces:**
- Consumes: Task 3 `UsageRatioController`, `UsageRatioValue`, `UsageRatioPoint`.
- Produces: `DashboardViewModel.ratioController: UsageRatioController`; `struct UsageRatioButton: View` with `static func label(_: UsageRatioValue) -> String`, `static func help(_: UsageRatioValue) -> String`; `struct UsageRatioChart: View` with `static func nearest(_: [UsageRatioPoint], to: Date) -> UsageRatioPoint?`.

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

@MainActor
final class UsageRatioChartTests: XCTestCase {
    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    func testLabelMeasured() {
        XCTAssertEqual(UsageRatioButton.label(UsageRatioValue(x: 16.27, updatedAt: t0)), "X 16.3%")
    }

    func testLabelDefault() {
        XCTAssertEqual(UsageRatioButton.label(UsageRatioValue(x: 16, updatedAt: nil)), "X 16% (default)")
    }

    func testHelpNamesTheValueAndTheUpdateTime() {
        let help = UsageRatioButton.help(UsageRatioValue(x: 16.27, updatedAt: t0))
        XCTAssertTrue(help.hasPrefix("100% of a 5h window costs about 16.3% of the 7d quota."), help)
        XCTAssertTrue(help.contains("updated hourly"), help)
    }

    func testHelpForTheDefaultSaysSo() {
        XCTAssertTrue(UsageRatioButton.help(UsageRatioValue(x: 16, updatedAt: nil)).contains("not enough usage history yet"))
    }

    func testNearestPoint() {
        let points = [0.0, 3600, 7200].map { UsageRatioPoint(date: t0.addingTimeInterval($0), x: $0, sumFiveHour: 0, sumSevenDay: 0) }
        XCTAssertEqual(UsageRatioChart.nearest(points, to: t0.addingTimeInterval(4000))?.x, 3600)
        XCTAssertNil(UsageRatioChart.nearest([], to: t0))
    }

    func testViewModelOwnsARatioController() async throws {
        let suite = StoreFixture.makeSuiteName()
        defer { StoreFixture.destroy(suite: suite) }
        let logPath = NSTemporaryDirectory() + "usage_\(UUID().uuidString).db"
        defer { try? FileManager.default.removeItem(atPath: logPath) }
        let vm = DashboardViewModel(accountStore: AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite))),
                                    logStore: UsageLogStore(dbPath: logPath))
        await vm.ratioController.recompute()   // empty logs: stays at the default
        XCTAssertEqual(vm.ratioController.current, UsageRatioValue(x: 16, updatedAt: nil))
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageRatioChartTests 2>&1 | tail -20
```
Expected: `cannot find 'UsageRatioButton' in scope`.

- [ ] **Step 3: Implement the views** (`Views/UsageRatioChart.swift`)

```swift
import SwiftUI
import Charts

/// Dashboard header button showing X; click opens its history chart.
///
///     UsageRatioButton(controller: vm.ratioController)
struct UsageRatioButton: View {
    @ObservedObject var controller: UsageRatioController
    @State private var showingChart = false

    var body: some View {
        Button { showingChart = true } label: {
            Text(Self.label(controller.current))
                .monospacedDigit()
                .foregroundStyle(controller.current.isDefault ? Color.secondary : Color.primary)
        }
        .help(Self.help(controller.current))
        .popover(isPresented: $showingChart, arrowEdge: .bottom) {
            UsageRatioChart(controller: controller)
                .frame(width: 440, height: 280)
                .padding(16)
        }
    }

    static func label(_ value: UsageRatioValue) -> String {
        value.isDefault ? "X \(Int(value.x.rounded()))% (default)" : String(format: "X %.1f%%", value.x)
    }

    static func help(_ value: UsageRatioValue) -> String {
        let head = String(format: "100%% of a 5h window costs about %.1f%% of the 7d quota.", value.x)
        guard let updatedAt = value.updatedAt else {
            return head + " Default value: not enough usage history yet. Click for the history."
        }
        let time = updatedAt.formatted(date: .omitted, time: .shortened)
        return head + " Measured over the last 3 days, updated hourly (last at \(time)). Click for the history."
    }
}

/// Line chart of X over 7 days, 30 days or everything kept (90 days).
struct UsageRatioChart: View {
    let controller: UsageRatioController
    @State private var range: Range = .week
    @State private var points: [UsageRatioPoint] = []
    @State private var hovered: UsageRatioPoint?

    enum Range: String, CaseIterable, Identifiable {
        case week = "7 days", month = "30 days", all = "All"
        var id: Self { self }
        var seconds: TimeInterval? {
            switch self {
            case .week: return 7 * 86_400
            case .month: return 30 * 86_400
            case .all: return nil
            }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("X over time").font(.headline)
                Spacer()
                Picker("Range", selection: $range) {
                    ForEach(Range.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 220)
            }
            if points.isEmpty {
                Text("No history yet")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                chart
                Text(hovered.map { String(format: "%@: X %.1f%%", $0.date.formatted(date: .abbreviated, time: .shortened), $0.x) } ?? " ")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        .task(id: range) {
            points = await controller.history(from: range.seconds.map { Date().addingTimeInterval(-$0) })
        }
    }

    private var chart: some View {
        Chart {
            ForEach(points, id: \.date) { p in
                LineMark(x: .value("Time", p.date), y: .value("X", p.x))
            }
            if let hovered {
                PointMark(x: .value("Time", hovered.date), y: .value("X", hovered.x))
            }
        }
        .chartYAxisLabel("% of 7d per full 5h")
        .chartOverlay { proxy in
            GeometryReader { geo in
                Rectangle().fill(.clear).contentShape(Rectangle())
                    .onContinuousHover { phase in
                        switch phase {
                        case .active(let location):
                            let x = location.x - geo[proxy.plotAreaFrame].origin.x
                            hovered = proxy.value(atX: x, as: Date.self).flatMap { Self.nearest(points, to: $0) }
                        case .ended:
                            hovered = nil
                        }
                    }
            }
        }
    }

    static func nearest(_ points: [UsageRatioPoint], to date: Date) -> UsageRatioPoint? {
        points.min { abs($0.date.timeIntervalSince(date)) < abs($1.date.timeIntervalSince(date)) }
    }
}
```

- [ ] **Step 4: Wire into the view model** (`DashboardViewModel.swift`)

Next to `let logStore: UsageLogStore` add:

```swift
    /// X, the 7d cost of a full 5h window; recomputed hourly (`UsageRatioController`).
    let ratioController: UsageRatioController
```

In `init`, right after `self.burnRateTracker = BurnRateTracker(logStore: store)`:

```swift
        self.ratioController = UsageRatioController.live(logStore: store, isRunningTests: AppDefaults.isRunningTests())
```

After the `pingController.objectWillChange` sink block:

```swift
        ratioController.objectWillChange
            .sink { [weak self] _ in self?.objectWillChange.send() }
            .store(in: &cancellables)
        ratioController.start()
```

- [ ] **Step 5: Show it on the Dashboard** (`DashboardPane.swift`, inside the `PaneHeader` trailing closure, before the `if !viewModel.accountStates.isEmpty` block):

```swift
                UsageRatioButton(controller: viewModel.ratioController)
```

- [ ] **Step 6: Run the tests and build**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageRatioChartTests 2>&1 | tail -20
```
Expected: 6 tests pass.

- [ ] **Step 7: Run check in the app**

Build and launch the Debug app (`xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build`, then open the built `.app`). Note: a Debug build uses the real stores (memory `feedback_dev_build_real_store`); this task only adds `ratio_logs.db`, which is new and safe. Verify: Dashboard header shows `X 16.x%` within a few seconds; hover shows the tooltip; click opens the popover with one point and the range picker. Take a screenshot.

- [ ] **Step 8: Commit**

```bash
git add apps/macos/ClaudeDashboard/Views/UsageRatioChart.swift apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift apps/macos/ClaudeDashboard/Views/DashboardPane.swift apps/macos/ClaudeDashboardTests/Critical/UsageRatioChartTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): show X on the Dashboard with its history chart"
```

---

# Part B: critical accounts

### Task 5: CriticalSettings

**Files:**
- Create: `apps/macos/ClaudeDashboard/Models/CriticalSettings.swift`
- Test: `apps/macos/ClaudeDashboardTests/Critical/CriticalSettingsTests.swift`

**Interfaces:**
- Consumes: Task 6's `QuietHours` and `CriticalParameters` (defined in `CriticalPlanner.swift`; write Task 6's type block first if executing out of order; the types are listed in Task 6 Step 3).
- Produces: `@MainActor final class CriticalSettings: ObservableObject { init(defaults: UserDefaults); @Published var pace: Int; @Published var highPace: Int; @Published var quietStart: Int; @Published var quietEnd: Int; @Published var timeZoneId: String; var quietHours: QuietHours?; func parameters(x: Double) -> CriticalParameters; static let paceRange, hourRange, paceStep, defaultTimeZone }`.

**Order note:** do Task 6 Step 3's type block (`QuietHours`, `CriticalLevel`, `CriticalAssessment`, `CriticalParameters`) as part of this task's Step 3 so this task compiles; Task 6 then adds the functions.

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

@MainActor
final class CriticalSettingsTests: XCTestCase {
    private var suite: String!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suite)
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        super.tearDown()
    }

    func testDefaults() {
        let s = CriticalSettings(defaults: defaults)
        XCTAssertEqual(s.pace, 50)
        XCTAssertEqual(s.highPace, 80)
        XCTAssertEqual(s.quietStart, 0)
        XCTAssertEqual(s.quietEnd, 7)
        XCTAssertEqual(s.timeZoneId, "Asia/Ho_Chi_Minh")
        XCTAssertEqual(s.quietHours, QuietHours(startHour: 0, endHour: 7, timeZone: TimeZone(identifier: "Asia/Ho_Chi_Minh")!))
    }

    func testValuesPersistAcrossInstances() {
        let s = CriticalSettings(defaults: defaults)
        s.pace = 40
        s.highPace = 90
        s.quietStart = 23
        s.quietEnd = 6
        s.timeZoneId = "Europe/Paris"
        let t = CriticalSettings(defaults: defaults)
        XCTAssertEqual([t.pace, t.highPace, t.quietStart, t.quietEnd], [40, 90, 23, 6])
        XCTAssertEqual(t.timeZoneId, "Europe/Paris")
    }

    func testRaisingPaceAboveHighPaceRaisesHighPace() {
        let s = CriticalSettings(defaults: defaults)
        s.pace = 85
        XCTAssertEqual(s.highPace, 85)
        XCTAssertEqual(CriticalSettings(defaults: defaults).highPace, 85)
    }

    func testHighPaceCannotGoBelowPace() {
        let s = CriticalSettings(defaults: defaults)
        s.highPace = 30
        XCTAssertEqual(s.highPace, 50)
    }

    func testInvalidStoredValuesFallBack() {
        defaults.set(5, forKey: CriticalSettings.paceKey)
        defaults.set(40, forKey: CriticalSettings.highPaceKey)
        defaults.set(30, forKey: CriticalSettings.quietStartKey)
        defaults.set(-1, forKey: CriticalSettings.quietEndKey)
        defaults.set("Mars/Olympus", forKey: CriticalSettings.timeZoneKey)
        let s = CriticalSettings(defaults: defaults)
        XCTAssertEqual(s.pace, 50)
        XCTAssertEqual(s.highPace, 50)   // stored 40 < pace: lifted to pace
        XCTAssertEqual(s.quietStart, 0)
        XCTAssertEqual(s.quietEnd, 7)
        XCTAssertEqual(s.timeZoneId, "Asia/Ho_Chi_Minh")
    }

    func testEqualStartAndEndMeansNoQuietHours() {
        let s = CriticalSettings(defaults: defaults)
        s.quietEnd = 0
        XCTAssertNil(s.quietHours)
    }

    func testParameters() {
        let p = CriticalSettings(defaults: defaults).parameters(x: 16.3)
        XCTAssertEqual(p, CriticalParameters(x: 16.3, pace: 50, highPace: 80,
                                             quiet: QuietHours(startHour: 0, endHour: 7, timeZone: TimeZone(identifier: "Asia/Ho_Chi_Minh")!)))
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/CriticalSettingsTests 2>&1 | tail -20
```
Expected: `cannot find 'CriticalSettings' in scope`.

- [ ] **Step 3: Implement** — first create `Services/Critical/CriticalPlanner.swift` with only the type block from Task 6 Step 3, then `Models/CriticalSettings.swift`:

```swift
import Foundation

/// Critical-account settings, persisted as soon as they change. The app passes
/// `AppDefaults.shared`; tests pass a suite of their own.
///
///     let settings = CriticalSettings(defaults: AppDefaults.shared)
///     settings.pace = 50                       // % of 5h per window, quiet hours excluded
///     settings.parameters(x: 16.3)             // feeds CriticalPlanner.assess
@MainActor
final class CriticalSettings: ObservableObject {
    static let paceKey = "criticalPace"
    static let highPaceKey = "criticalHighPace"
    static let quietStartKey = "criticalQuietStart"
    static let quietEndKey = "criticalQuietEnd"
    static let timeZoneKey = "criticalTimeZone"
    static let paceRange = 10...100
    static let paceStep = 5
    static let hourRange = 0...23
    static let defaultTimeZone = "Asia/Ho_Chi_Minh"

    private let defaults: UserDefaults

    /// Raising it above `highPace` raises `highPace` with it.
    @Published var pace: Int {
        didSet {
            defaults.set(pace, forKey: Self.paceKey)
            if highPace < pace { highPace = pace }
        }
    }
    /// Never below `pace`.
    @Published var highPace: Int {
        didSet {
            if highPace < pace { highPace = pace }
            defaults.set(highPace, forKey: Self.highPaceKey)
        }
    }
    @Published var quietStart: Int {
        didSet { defaults.set(quietStart, forKey: Self.quietStartKey) }
    }
    @Published var quietEnd: Int {
        didSet { defaults.set(quietEnd, forKey: Self.quietEndKey) }
    }
    @Published var timeZoneId: String {
        didSet { defaults.set(timeZoneId, forKey: Self.timeZoneKey) }
    }

    init(defaults: UserDefaults) {
        self.defaults = defaults
        let pace = Self.stored(defaults, Self.paceKey, in: Self.paceRange, default: 50)
        self.pace = pace
        highPace = max(Self.stored(defaults, Self.highPaceKey, in: Self.paceRange, default: 80), pace)
        quietStart = Self.stored(defaults, Self.quietStartKey, in: Self.hourRange, default: 0)
        quietEnd = Self.stored(defaults, Self.quietEndKey, in: Self.hourRange, default: 7)
        let zone = defaults.string(forKey: Self.timeZoneKey)
        timeZoneId = zone.flatMap(TimeZone.init(identifier:)) != nil ? zone! : Self.defaultTimeZone
    }

    /// nil when start == end (no quiet hours).
    var quietHours: QuietHours? {
        guard quietStart != quietEnd else { return nil }
        let zone = TimeZone(identifier: timeZoneId) ?? TimeZone(identifier: Self.defaultTimeZone)!
        return QuietHours(startHour: quietStart, endHour: quietEnd, timeZone: zone)
    }

    func parameters(x: Double) -> CriticalParameters {
        CriticalParameters(x: x, pace: Double(pace), highPace: Double(highPace), quiet: quietHours)
    }

    private static func stored(_ defaults: UserDefaults, _ key: String,
                               in range: ClosedRange<Int>, default value: Int) -> Int {
        guard let stored = defaults.object(forKey: key) as? Int, range.contains(stored) else { return value }
        return stored
    }
}
```

- [ ] **Step 4: Run to verify they pass** (same command). Expected: 7 tests pass.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Models/CriticalSettings.swift apps/macos/ClaudeDashboard/Services/Critical/CriticalPlanner.swift apps/macos/ClaudeDashboardTests/Critical/CriticalSettingsTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): critical account settings (paces, quiet hours, time zone)"
```

### Task 6: CriticalPlanner

**Files:**
- Modify (created in Task 5): `apps/macos/ClaudeDashboard/Services/Critical/CriticalPlanner.swift`
- Test: `apps/macos/ClaudeDashboardTests/Critical/CriticalPlannerTests.swift`

**Interfaces:**
- Consumes: `UsageData`, `UsageLimit` (`Shared/UsageData.swift`).
- Produces: `struct QuietHours: Equatable { startHour, endHour: Int; timeZone: TimeZone }`; `enum CriticalLevel: Equatable { case none, critical, high; var label: String }`; `struct CriticalAssessment: Equatable { level; waste: Double; static let none }`; `struct CriticalParameters: Equatable { x, pace, highPace: Double; quiet: QuietHours? }`; `struct CriticalBadge: Equatable { level: CriticalLevel; help: String }`; `enum CriticalPlanner { static func activeHours(from: Date, to: Date, quiet: QuietHours?) -> Double; static func projected(sevenDay:activeHours:pace:x:) -> Double; static func assess(_: UsageData?, params: CriticalParameters, now: Date) -> CriticalAssessment; static func badge(for: CriticalAssessment, params: CriticalParameters) -> CriticalBadge? }`.

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

final class CriticalPlannerTests: XCTestCase {
    private let vn = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
    private let ny = TimeZone(identifier: "America/New_York")!

    /// A wall-clock time in `zone`.
    private func at(_ y: Int, _ m: Int, _ d: Int, _ h: Int, _ min: Int = 0, in zone: TimeZone) -> Date {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = zone
        return cal.date(from: DateComponents(year: y, month: m, day: d, hour: h, minute: min))!
    }

    private var quietVN: QuietHours { QuietHours(startHour: 0, endHour: 7, timeZone: vn) }

    private func params(x: Double = 16, quiet: QuietHours?) -> CriticalParameters {
        CriticalParameters(x: x, pace: 50, highPace: 80, quiet: quiet)
    }

    private func usage(seven: Double, resets: Date?) -> UsageData {
        UsageData(fiveHour: UsageLimit(utilization: 0, resetsAt: nil),
                  sevenDay: UsageLimit(utilization: seven, resetsAt: resets))
    }

    // MARK: activeHours

    func testNoQuietHoursCountsEverything() {
        let now = at(2026, 10, 16, 10, in: vn)
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: now.addingTimeInterval(48 * 3600), quiet: nil), 48, accuracy: 1e-9)
    }

    func testTwoDaysLoseTwoQuietBlocks() {
        let now = at(2026, 10, 16, 10, in: vn)
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: now.addingTimeInterval(48 * 3600), quiet: quietVN), 34, accuracy: 1e-9)
    }

    func testStartingInsideQuietHours() {
        let now = at(2026, 10, 16, 3, in: vn)     // 4h of quiet left, then 07:00 to 12:00
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: at(2026, 10, 16, 12, in: vn), quiet: quietVN), 5, accuracy: 1e-9)
    }

    func testQuietHoursCrossingMidnight() {
        let q = QuietHours(startHour: 22, endHour: 6, timeZone: vn)
        let now = at(2026, 10, 16, 20, in: vn)
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: at(2026, 10, 17, 8, in: vn), quiet: q), 4, accuracy: 1e-9)
    }

    func testQuietHoursUseTheirOwnTimeZone() {
        // 10:00 to 20:00 Vietnam time is 23:00 to 09:00 New York time (EDT). With quiet hours
        // 00:00 to 07:00 *New York*, 7 of the 10 hours are quiet. An implementation that reads
        // quiet hours in the Mac's zone (Vietnam on this machine) would find none.
        let q = QuietHours(startHour: 0, endHour: 7, timeZone: ny)
        let now = at(2026, 10, 16, 10, in: vn)
        let hours = CriticalPlanner.activeHours(from: now, to: at(2026, 10, 16, 20, in: vn), quiet: q)
        XCTAssertEqual(hours, 3, accuracy: 1e-9)
    }

    func testDaylightSavingDayInNewYork() {
        // 2026-11-01 is 25h long in New York; quiet 00:00 to 07:00 on that day is 8 real hours.
        let q = QuietHours(startHour: 0, endHour: 7, timeZone: ny)
        let start = at(2026, 10, 31, 12, in: ny)
        let end = at(2026, 11, 1, 12, in: ny)
        XCTAssertEqual(end.timeIntervalSince(start) / 3600, 25, accuracy: 1e-9)
        XCTAssertEqual(CriticalPlanner.activeHours(from: start, to: end, quiet: q), 17, accuracy: 1e-9)
    }

    func testEndBeforeStartIsZero() {
        let now = at(2026, 10, 16, 10, in: vn)
        XCTAssertEqual(CriticalPlanner.activeHours(from: now, to: now.addingTimeInterval(-60), quiet: quietVN), 0)
    }

    // MARK: assess

    /// Spec example: X 16, pace 50 (8% per window), 7d 55%, 25 active hours left: 55 + 40 = 95.
    func testSpecExampleIsCritical() {
        let now = at(2026, 10, 16, 8, in: vn)
        let a = CriticalPlanner.assess(usage(seven: 55, resets: now.addingTimeInterval(25 * 3600)),
                                       params: params(quiet: nil), now: now)
        // No quiet hours: high pace 80 -> 12.8% per window, 55 + 64 = 119 (not high); pace 50 -> 95.
        XCTAssertEqual(a.level, .critical)
        XCTAssertEqual(a.waste, 5, accuracy: 1e-9)
    }

    func testHighWhenEvenTheHighPaceAroundTheClockCannotFinish() {
        let now = at(2026, 10, 16, 8, in: vn)
        // 10h left (2 windows) at 80 -> 25.6; 7d 60 -> 85.6.
        let a = CriticalPlanner.assess(usage(seven: 60, resets: now.addingTimeInterval(10 * 3600)),
                                       params: params(quiet: quietVN), now: now)
        XCTAssertEqual(a.level, .high)
        XCTAssertEqual(a.waste, 14.4, accuracy: 1e-9)
    }

    func testQuietHoursCanMakeAnAccountCritical() {
        let now = at(2026, 10, 16, 10, in: vn)
        let u = usage(seven: 60, resets: now.addingTimeInterval(48 * 3600))
        XCTAssertEqual(CriticalPlanner.assess(u, params: params(quiet: nil), now: now).level, .none)  // 60 + 76.8
        let a = CriticalPlanner.assess(u, params: params(quiet: quietVN), now: now)                   // 60 + 54.4
        XCTAssertEqual(a.level, .none)
        let late = usage(seven: 50, resets: now.addingTimeInterval(48 * 3600))                       // 50 + 54.4
        XCTAssertEqual(CriticalPlanner.assess(late, params: params(quiet: quietVN), now: now).level, .none)
        let low = usage(seven: 40, resets: now.addingTimeInterval(48 * 3600))                        // 40 + 54.4
        XCTAssertEqual(CriticalPlanner.assess(low, params: params(quiet: quietVN), now: now).level, .critical)
    }

    func testExactlyOneHundredCounts() {
        let now = at(2026, 10, 16, 8, in: vn)
        // 25h, pace 50, X 16: 40 -> 7d 60 lands on exactly 100.
        let a = CriticalPlanner.assess(usage(seven: 60, resets: now.addingTimeInterval(25 * 3600)),
                                       params: params(quiet: nil), now: now)
        XCTAssertEqual(a.level, .critical)
        XCTAssertEqual(a.waste, 0, accuracy: 1e-9)
    }

    func testWeekNotStartedIsNotCritical() {
        let now = at(2026, 10, 16, 8, in: vn)
        XCTAssertEqual(CriticalPlanner.assess(usage(seven: 0, resets: nil), params: params(quiet: nil), now: now), .none)
    }

    func testMissingUsageIsNotCritical() {
        XCTAssertEqual(CriticalPlanner.assess(nil, params: params(quiet: nil), now: Date()), .none)
    }

    func testExhaustedWeekIsNotCritical() {
        let now = at(2026, 10, 16, 8, in: vn)
        XCTAssertEqual(CriticalPlanner.assess(usage(seven: 100, resets: now.addingTimeInterval(3600)),
                                              params: params(quiet: nil), now: now), .none)
        XCTAssertEqual(CriticalPlanner.assess(usage(seven: 99.5, resets: now.addingTimeInterval(3600)),
                                              params: params(quiet: nil), now: now), .none)
    }

    // MARK: badge

    func testBadgeText() {
        let p = params(quiet: quietVN)
        XCTAssertNil(CriticalPlanner.badge(for: .none, params: p))
        XCTAssertEqual(CriticalPlanner.badge(for: CriticalAssessment(level: .high, waste: 12.4), params: p),
                       CriticalBadge(level: .high, help: "High critical: at 80% of 5h per window around the clock, 7d reaches 88% by its reset (12% unused)"))
        XCTAssertEqual(CriticalPlanner.badge(for: CriticalAssessment(level: .critical, waste: 5), params: p),
                       CriticalBadge(level: .critical, help: "Critical: at 50% of 5h per window outside quiet hours (00:00 to 07:00), 7d reaches 95% by its reset (5% unused)"))
    }
}
```

The exhausted-week rule: an account is not assessed when `7d >= 99`, matching auto switch's `tierSevenDayLimit` (a week at 99%+ has nothing meaningful left to burn).

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/CriticalPlannerTests 2>&1 | tail -20
```
Expected: `type 'CriticalPlanner' has no member 'activeHours'` (types exist from Task 5).

- [ ] **Step 3: Implement** — the full `CriticalPlanner.swift` (the type block at the top is what Task 5 created):

```swift
import Foundation

/// Daily hours not counted as usable time, as whole hours in `timeZone`. A start after the
/// end crosses midnight (22 to 6).
struct QuietHours: Equatable {
    let startHour: Int
    let endHour: Int
    let timeZone: TimeZone
}

enum CriticalLevel: Equatable {
    case none, critical, high

    var label: String {
        switch self {
        case .none: return "not critical"
        case .critical: return "critical"
        case .high: return "high critical"
        }
    }
}

/// `waste`: the 7d percent still unused at the reset under the level's own pace.
struct CriticalAssessment: Equatable {
    let level: CriticalLevel
    let waste: Double
    static let none = CriticalAssessment(level: .none, waste: 0)
}

struct CriticalParameters: Equatable {
    let x: Double
    let pace: Double
    let highPace: Double
    let quiet: QuietHours?
}

struct CriticalBadge: Equatable {
    let level: CriticalLevel
    let help: String
}

/// Whether an account's 7d quota will go unused. Pure; spec
/// `docs/superpowers/specs/2026-10-16-critical-accounts-design.md`, "Assessment".
///
///     let a = CriticalPlanner.assess(usage, params: settings.parameters(x: 16.3), now: Date())
///     a.level   // .critical
enum CriticalPlanner {
    static let windowHours = 5.0
    /// A week at or above this has nothing meaningful left; matches auto switch's 7d tier limit.
    static let sevenDayLimit = 99.0

    /// Hours in `[from, to)` outside the quiet hours.
    static func activeHours(from start: Date, to end: Date, quiet: QuietHours?) -> Double {
        guard end > start else { return 0 }
        var seconds = end.timeIntervalSince(start)
        guard let quiet, quiet.startHour != quiet.endHour else { return seconds / 3600 }
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = quiet.timeZone
        // Start a day early: a block that began yesterday evening may still run.
        var day = cal.date(byAdding: .day, value: -1, to: cal.startOfDay(for: start))!
        while day < end {
            let blockStart = cal.date(bySettingHour: quiet.startHour, minute: 0, second: 0, of: day)!
            let endDay = quiet.endHour > quiet.startHour ? day : cal.date(byAdding: .day, value: 1, to: day)!
            let blockEnd = cal.date(bySettingHour: quiet.endHour, minute: 0, second: 0, of: endDay)!
            let overlap = min(blockEnd, end).timeIntervalSince(max(blockStart, start))
            if overlap > 0 { seconds -= overlap }
            day = cal.date(byAdding: .day, value: 1, to: day)!
        }
        return max(seconds, 0) / 3600
    }

    /// 7d at the reset if each 5h window from now uses `pace`% of its limit.
    static func projected(sevenDay: Double, activeHours: Double, pace: Double, x: Double) -> Double {
        sevenDay + activeHours / windowHours * pace / 100 * x
    }

    static func assess(_ usage: UsageData?, params: CriticalParameters, now: Date) -> CriticalAssessment {
        guard let usage, let resetsAt = usage.sevenDay.resetsAt, resetsAt > now, params.x > 0,
              usage.sevenDay.utilization < sevenDayLimit else { return .none }
        let seven = usage.sevenDay.utilization
        let high = projected(sevenDay: seven, activeHours: activeHours(from: now, to: resetsAt, quiet: nil),
                             pace: params.highPace, x: params.x)
        if high <= 100 { return CriticalAssessment(level: .high, waste: 100 - high) }
        let normal = projected(sevenDay: seven, activeHours: activeHours(from: now, to: resetsAt, quiet: params.quiet),
                               pace: params.pace, x: params.x)
        if normal <= 100 { return CriticalAssessment(level: .critical, waste: 100 - normal) }
        return .none
    }

    static func badge(for assessment: CriticalAssessment, params: CriticalParameters) -> CriticalBadge? {
        let reach = Int((100 - assessment.waste).rounded())
        let unused = Int(assessment.waste.rounded())
        switch assessment.level {
        case .none:
            return nil
        case .high:
            return CriticalBadge(level: .high, help: "High critical: at \(Int(params.highPace))% of 5h per window around the clock, 7d reaches \(reach)% by its reset (\(unused)% unused)")
        case .critical:
            let quiet = params.quiet.map { String(format: " outside quiet hours (%02d:00 to %02d:00)", $0.startHour, $0.endHour) } ?? " around the clock"
            return CriticalBadge(level: .critical, help: "Critical: at \(Int(params.pace))% of 5h per window\(quiet), 7d reaches \(reach)% by its reset (\(unused)% unused)")
        }
    }
}
```

- [ ] **Step 4: Run to verify they pass** (same command). Expected: 15 tests pass. If `testDaylightSavingDayInNewYork` fails, check `bySettingHour` on the fall-back day returns the first 01:00 occurrence; the quiet block is 00:00 to 07:00 local, which spans 8 real hours that day.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/Critical/CriticalPlanner.swift apps/macos/ClaudeDashboardTests/Critical/CriticalPlannerTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): assess critical and high critical accounts"
```

### Task 7: Auto switch rules for critical accounts

**Files:**
- Modify: `apps/macos/ClaudeDashboard/Services/AutoSwitch/AutoSwitchPlanner.swift` (`AutoSwitchAccount`, `AutoSwitchReason`, `decide`)
- Modify: `apps/macos/ClaudeDashboard/Services/AutoSwitch/AutoSwitchController.swift:17` and `:105-108`
- Test: `apps/macos/ClaudeDashboardTests/AutoSwitch/AutoSwitchPlannerTests.swift`, `AutoSwitchControllerTests.swift`

**Interfaces:**
- Consumes: Task 6 `CriticalAssessment`, `CriticalLevel`.
- Produces: `AutoSwitchAccount.critical: CriticalAssessment` (var, default `.none`); `AutoSwitchReason.critical(CriticalLevel)`; `AutoSwitchPlanner.freeFiveHourLimit = 2.0`; `AutoSwitchPlanner.isCriticalTarget(_:excluded:) -> Bool`.

- [ ] **Step 1: Write the failing planner tests** — append to `AutoSwitchPlannerTests` (it already has `account(...)`, `snapshot(...)`, `decide(...)`, `now`, `day`):

```swift
    // MARK: Critical

    private func critical(_ a: AutoSwitchAccount, _ level: CriticalLevel, waste: Double = 10) -> AutoSwitchAccount {
        var a = a
        a.critical = CriticalAssessment(level: level, waste: waste)
        return a
    }

    func testHighCriticalPullsFromAnOrdinaryAccountBelowThreshold() {
        let active = account("a", five: 10, seven: 10, active: true)
        let target = critical(account("b", five: 0, seven: 30), .high)
        XCTAssertEqual(decide(snapshot([active, target])), .switchTo(target.id, .critical(.high)))
    }

    func testHighCriticalPullsFromHome() {
        let active = account("home", five: 10, seven: 10, active: true)
        let target = critical(account("b", five: 0, seven: 30), .high)
        let s = snapshot([active, target], priority: AutoSwitchPriority(accountId: active.id, baseline: nil))
        XCTAssertEqual(decide(s), .switchTo(target.id, .critical(.high)))
    }

    func testCriticalDoesNotPullFromHome() {
        let active = account("home", five: 10, seven: 10, active: true)
        let target = critical(account("b", five: 0, seven: 30), .critical)
        let s = snapshot([active, target], priority: AutoSwitchPriority(accountId: active.id, baseline: nil))
        XCTAssertEqual(decide(s), .stay(.belowThreshold))
    }

    func testCriticalPullsFromAnOrdinaryAccount() {
        let active = account("a", five: 10, seven: 10, active: true)
        let target = critical(account("b", five: 0, seven: 30), .critical)
        XCTAssertEqual(decide(snapshot([active, target])), .switchTo(target.id, .critical(.critical)))
    }

    func testCriticalDoesNotPullFromAnotherCritical() {
        let active = critical(account("a", five: 10, seven: 10, active: true), .critical)
        let target = critical(account("b", five: 0, seven: 30), .critical, waste: 50)
        XCTAssertEqual(decide(snapshot([active, target])), .stay(.belowThreshold))
    }

    func testHighCriticalDoesNotPullFromAnotherHighCritical() {
        let active = critical(account("a", five: 10, seven: 10, active: true), .high)
        let target = critical(account("b", five: 0, seven: 30), .high, waste: 50)
        XCTAssertEqual(decide(snapshot([active, target])), .stay(.belowThreshold))
    }

    func testHighCriticalPullsFromCritical() {
        let active = critical(account("a", five: 10, seven: 10, active: true), .critical)
        let target = critical(account("b", five: 0, seven: 30), .high)
        XCTAssertEqual(decide(snapshot([active, target])), .switchTo(target.id, .critical(.high)))
    }

    func testHomeResetPullsFromCritical() {
        let active = critical(account("a", five: 10, seven: 10, active: true), .critical)
        let home = account("home", five: 0, seven: 10)
        let s = snapshot([active, home], priority: AutoSwitchPriority(accountId: home.id, baseline: nil))
        XCTAssertEqual(decide(s), .switchTo(home.id, .priorityReset))
    }

    func testHomeResetDoesNotPullFromHighCritical() {
        let active = critical(account("a", five: 10, seven: 10, active: true), .high)
        let home = account("home", five: 0, seven: 10)
        let s = snapshot([active, home], priority: AutoSwitchPriority(accountId: home.id, baseline: nil))
        XCTAssertEqual(decide(s), .stay(.belowThreshold))
    }

    func testHighCriticalBeatsAHomeResetAtTheSameTime() {
        let active = account("a", five: 10, seven: 10, active: true)
        let home = account("home", five: 0, seven: 10)
        let target = critical(account("b", five: 0, seven: 30), .high)
        let s = snapshot([active, home, target], priority: AutoSwitchPriority(accountId: home.id, baseline: nil))
        XCTAssertEqual(decide(s), .switchTo(target.id, .critical(.high)))
    }

    func testHomeBeatsCriticalAtTheSameTime() {
        let active = account("a", five: 10, seven: 10, active: true)
        let home = account("home", five: 0, seven: 10)
        let target = critical(account("b", five: 0, seven: 30), .critical)
        let s = snapshot([active, home, target], priority: AutoSwitchPriority(accountId: home.id, baseline: nil))
        XCTAssertEqual(decide(s), .switchTo(home.id, .priorityReset))
    }

    func testFiveHourAtTwoIsFreeAndAtThreeIsNot() {
        let active = account("a", five: 10, seven: 10, active: true)
        let two = critical(account("b", five: 2, seven: 30, fiveResets: now.addingTimeInterval(3600)), .critical)
        XCTAssertEqual(decide(snapshot([active, two])), .switchTo(two.id, .critical(.critical)))
        let three = critical(account("c", five: 3, seven: 30, fiveResets: now.addingTimeInterval(3600)), .critical)
        XCTAssertEqual(decide(snapshot([active, three])), .stay(.belowThreshold))
    }

    func testLargestWasteWins() {
        let active = account("a", five: 10, seven: 10, active: true)
        let small = critical(account("b", five: 0, seven: 30), .critical, waste: 3)
        let large = critical(account("c", five: 0, seven: 30), .critical, waste: 9)
        XCTAssertEqual(decide(snapshot([active, small, large])), .switchTo(large.id, .critical(.critical)))
    }

    func testCriticalTargetMustBeACandidate() {
        let active = account("a", five: 10, seven: 10, active: true)
        let notReady = critical(account("b", five: 0, seven: 30, availability: .notCaptured), .high)
        var stale = critical(account("c", five: 0, seven: 30), .high)
        stale.isFresh = false
        let cooling = critical(account("d", five: 0, seven: 30), .high)
        XCTAssertEqual(decide(snapshot([active, notReady, stale, cooling], excluded: [cooling.id])), .stay(.belowThreshold))
    }

    /// After the switch the target is active and its 5h runs: the next evaluation stays.
    func testNoPingPongAfterTheSwitch() {
        let previous = account("a", five: 10, seven: 10)
        let nowActive = critical(account("b", five: 5, seven: 31, fiveResets: now.addingTimeInterval(4 * 3600), active: true), .critical)
        XCTAssertEqual(decide(snapshot([previous, nowActive])), .stay(.belowThreshold))
    }

    func testThresholdTiersStillRunWhenNoCriticalTarget() {
        let active = account("a", five: 96, seven: 10, active: true)
        let other = account("b", five: 0, seven: 10)
        XCTAssertEqual(decide(snapshot([active, other])), .switchTo(other.id, .threshold(tier: 1)))
    }
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/AutoSwitchPlannerTests 2>&1 | tail -20
```
Expected: `value of type 'AutoSwitchAccount' has no member 'critical'`.

- [ ] **Step 3: Implement the planner change**

In `AutoSwitchAccount`, after `var isFresh = true`:

```swift
    /// From `CriticalPlanner.assess`; `.none` when not critical or unknown.
    var critical: CriticalAssessment = .none
```

Replace `AutoSwitchReason`:

```swift
enum AutoSwitchReason: Equatable {
    case priorityReset
    case critical(CriticalLevel)
    case threshold(tier: Int)
}
```

Add to `AutoSwitchPlanner` constants:

```swift
    /// A critical target's 5h counts as free at or under this (an Auto Ping leaves about 1%).
    static let freeFiveHourLimit = 2.0
```

Add next to `isCandidate`:

```swift
    /// A critical candidate whose 5h window is free and whose week is not used up.
    static func isCriticalTarget(_ a: AutoSwitchAccount, excluded: Set<UUID>) -> Bool {
        guard a.critical.level != .none, isCandidate(a, excluded: excluded),
              a.sevenDay < tierSevenDayLimit, let usage = a.usage else { return false }
        return usage.fiveHour.resetsAt == nil || usage.fiveHour.utilization <= freeFiveHourLimit
    }
```

In `decide`, replace the block from `let candidates = ...` through the end of the priority `if` with:

```swift
        let candidates = s.accounts.filter { isCandidate($0, excluded: s.excluded) }
        let targets = s.accounts.filter { isCriticalTarget($0, excluded: s.excluded) }
        let activeIsHigh = active.critical.level == .high

        // 1. High critical beats everything, home included.
        if !activeIsHigh,
           let target = targets.filter({ $0.critical.level == .high }).max(by: { $0.critical.waste < $1.critical.waste }) {
            return .switchTo(target.id, .critical(.high))
        }

        // 2. Home, unless Claude Code is on a high critical account.
        if !activeIsHigh, let priority = s.priority,
           let target = candidates.first(where: { $0.id == priority.accountId }),
           target.sevenDay < tierSevenDayLimit,
           isPriorityDue(current: target.usage?.fiveHour.resetsAt, baseline: priority.baseline, now: s.now) {
            return .switchTo(target.id, .priorityReset)
        }

        // 3. Critical beats an ordinary account that is not home.
        if active.critical.level == .none, active.id != s.priority?.accountId,
           let target = targets.max(by: { $0.critical.waste < $1.critical.waste }) {
            return .switchTo(target.id, .critical(target.critical.level))
        }
```

(Step 3 only reaches `.critical` targets in practice: any `.high` target was taken by step 1, since the active account is `.none`.)

Update the doc comment above `enum AutoSwitchPlanner` to name the order: "high critical, home, critical, then the threshold tiers".

- [ ] **Step 4: Run the planner tests** (same command). Expected: all pass, old and new.

- [ ] **Step 5: Write the failing controller test** — append to `AutoSwitchControllerTests` (follow that file's existing helpers for building the controller, settings and accounts; it already builds `AutoSwitchAccount` values and reads `lastEvent` and the log store):

```swift
    func testCriticalSwitchTextInFooterAndLog() async {
        let active = account("a@x.com", five: 10, seven: 10, active: true)
        var target = account("b@x.com", five: 0, seven: 30)
        target.critical = CriticalAssessment(level: .high, waste: 12.4)

        let result = await controller.evaluate([active, target]) { _ in }

        XCTAssertEqual(result, target.id)
        XCTAssertEqual(controller.lastEvent?.kind, .switched(.critical(.high)))
        XCTAssertEqual(controller.lastEvent?.text, "Switched to b@x.com: high critical account")
        let row = await AutoSwitchLogStore(dbPath: logPath).fetch(limit: 1).first
        XCTAssertEqual(row?.reason, "High critical account's 5h window is free, 12% of 7d would go unused")
    }
```

- [ ] **Step 6: Run to verify it fails**

```bash
cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/AutoSwitchControllerTests 2>&1 | tail -20
```
Expected: compile error, switch must be exhaustive (`.critical` not handled) in `AutoSwitchController.swift`.

- [ ] **Step 7: Implement the controller text**

In `AutoSwitchEvent.text`, add before `case .switched:`:

```swift
        case .switched(.critical(let level)): return "Switched to \(name): \(level.label) account"
```

In `evaluate`, replace the `reasonText` switch with:

```swift
            switch reason {
            case .priorityReset: reasonText = "Ticked account's 5h window reset"
            case .critical(let level):
                let kind = level == .high ? "High critical" : "Critical"
                let waste = Self.percent(target?.critical.waste ?? 0)
                reasonText = "\(kind) account's 5h window is free, \(waste) of 7d would go unused"
            case .threshold(let tier): reasonText = "\(triggerText(active)), tier \(tier)"
            }
```

- [ ] **Step 8: Run all auto switch tests**

```bash
cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/AutoSwitchPlannerTests -only-testing:ClaudeDashboardTests/AutoSwitchControllerTests 2>&1 | tail -20
```
Expected: all pass.

- [ ] **Step 9: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/AutoSwitch apps/macos/ClaudeDashboardTests/AutoSwitch
git commit -m "feat(macos): auto switch to critical accounts (high critical > home > critical)"
```

### Task 8: Wire critical into the view model and cards

**Files:**
- Create: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel+Critical.swift`
- Modify: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift` (property, init param, forward changes)
- Modify: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel+AutoSwitch.swift:9-15`
- Modify: `apps/macos/ClaudeDashboard/Views/AccountCard.swift` (property + badge before the bolt)
- Modify: `apps/macos/ClaudeDashboard/Views/DashboardPane.swift`, `apps/macos/ClaudeDashboard/Views/MenuBarPopover.swift` (pass `critical:`)
- Test: `apps/macos/ClaudeDashboardTests/Critical/CriticalWiringTests.swift`

**Interfaces:**
- Consumes: Tasks 3, 5, 6, 7.
- Produces: `DashboardViewModel.criticalSettings: CriticalSettings`; `func criticalAssessment(for: AccountUsageState, now: Date = Date()) -> CriticalAssessment`; `func criticalBadge(for: AccountUsageState) -> CriticalBadge?`; `AccountCard.critical: CriticalBadge?`.

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

@MainActor
final class CriticalWiringTests: XCTestCase {
    private var suite: String!
    private var defaults: UserDefaults!
    private var logPath: String!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suite)
        logPath = NSTemporaryDirectory() + "usage_\(UUID().uuidString).db"
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(atPath: logPath)
        super.tearDown()
    }

    /// An empty usage log keeps X at the default 16 whatever the hourly job does.
    private func makeViewModel() -> DashboardViewModel {
        DashboardViewModel(accountStore: AccountStore(defaults: defaults),
                           logStore: UsageLogStore(dbPath: logPath),
                           criticalSettings: CriticalSettings(defaults: defaults))
    }

    private func state(seven: Double, hoursLeft: Double, error: String? = nil) -> AccountUsageState {
        let account = Account(id: UUID(), name: "a", email: "a@x.com", chromeProfilePath: "Profile 1",
                              chromeProfileName: nil, orgId: nil, accountUuid: nil, plan: .max5x,
                              lastSynced: nil, status: .active, isPinned: false, source: .browser)
        let usage = UsageData(fiveHour: UsageLimit(utilization: 0, resetsAt: nil),
                              sevenDay: UsageLimit(utilization: seven, resetsAt: Date().addingTimeInterval(hoursLeft * 3600)))
        return AccountUsageState(id: account.id, account: account, usage: usage, error: error)
    }

    func testTenHoursLeftAtSixtyIsHighCritical() {
        let vm = makeViewModel()
        // X default 16, high pace 80: 2 windows x 12.8 = 25.6 -> 85.6.
        XCTAssertEqual(vm.criticalAssessment(for: state(seven: 60, hoursLeft: 10)).level, .high)
        XCTAssertEqual(vm.criticalBadge(for: state(seven: 60, hoursLeft: 10))?.level, .high)
    }

    func testBusyAccountIsNotCritical() {
        let vm = makeViewModel()
        XCTAssertEqual(vm.criticalAssessment(for: state(seven: 30, hoursLeft: 100)), .none)
        XCTAssertNil(vm.criticalBadge(for: state(seven: 30, hoursLeft: 100)))
    }

    func testStaleUsageIsNotCritical() {
        let vm = makeViewModel()
        XCTAssertEqual(vm.criticalAssessment(for: state(seven: 60, hoursLeft: 10, error: "HTTP 500")), .none)
    }

    func testAutoSwitchAccountsCarryTheAssessment() {
        let vm = makeViewModel()
        let s = state(seven: 60, hoursLeft: 10)
        vm.accountStates = [s]
        XCTAssertEqual(vm.autoSwitchAccounts.first?.critical.level, .high)
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/CriticalWiringTests 2>&1 | tail -20
```
Expected: `extra argument 'criticalSettings' in call`.

- [ ] **Step 3: Implement the view model parts**

`DashboardViewModel.swift`: below `let autoSwitch: AutoSwitchController` add

```swift
    /// Paces and quiet hours for critical accounts (`DashboardViewModel+Critical`).
    let criticalSettings: CriticalSettings
```

Add init parameter after `pingController: PingController? = nil` (comma on the line before):

```swift
        criticalSettings: CriticalSettings? = nil
```

In the init body after `self.autoSwitch = ...`:

```swift
        self.criticalSettings = criticalSettings ?? CriticalSettings(defaults: AppDefaults.shared)
```

Next to the other `objectWillChange` forwards:

```swift
        self.criticalSettings.objectWillChange
            .sink { [weak self] _ in self?.objectWillChange.send() }
            .store(in: &cancellables)
```

Create `DashboardViewModel+Critical.swift`:

```swift
import Foundation

/// Critical glue: one assessment per account state from the current X and settings.
/// Rules live in `CriticalPlanner`; this file only feeds it.
extension DashboardViewModel {

    /// `.none` for a stale fetch or an account that is not active.
    func criticalAssessment(for state: AccountUsageState, now: Date = Date()) -> CriticalAssessment {
        guard state.error == nil, state.account.status == .active else { return .none }
        return CriticalPlanner.assess(state.usage, params: criticalParameters, now: now)
    }

    func criticalBadge(for state: AccountUsageState) -> CriticalBadge? {
        CriticalPlanner.badge(for: criticalAssessment(for: state), params: criticalParameters)
    }

    private var criticalParameters: CriticalParameters {
        criticalSettings.parameters(x: ratioController.current.x)
    }
}
```

In `DashboardViewModel+AutoSwitch.swift`, add the argument after `isFresh: state.error == nil`:

```swift
                              isFresh: state.error == nil,
                              critical: criticalAssessment(for: state))
```

- [ ] **Step 4: Run to verify they pass** (same command). Expected: 4 tests pass.

- [ ] **Step 5: Add the badge to `AccountCard`**

Property, after `var onTogglePriority`:

```swift
    /// Non-nil shows the flame: orange for critical, red for high critical.
    var critical: CriticalBadge? = nil
```

Inside the trailing `HStack(spacing: 2) {`, as its first child (before `if let onTogglePriority`):

```swift
                        if let critical {
                            Image(systemName: critical.level == .high ? "flame.fill" : "flame")
                                .font(.callout)
                                .foregroundStyle(critical.level == .high ? Color.red : Color.orange)
                                .padding(.horizontal, 3)
                                .help(critical.help)
                                .accessibilityLabel(critical.level.label)
                        }
```

In `DashboardPane.swift` and `MenuBarPopover.swift`, add to the `AccountCard(` call right after the `onTogglePriority:` argument:

```swift
                                    critical: viewModel.criticalBadge(for: state),
```

- [ ] **Step 6: Build, run the full app suite**

```bash
cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests 2>&1 | tail -30
```
Expected: `** TEST SUCCEEDED **`. `wc -l ClaudeDashboard/ViewModels/DashboardViewModel.swift` stays under 980.

- [ ] **Step 7: Run check in the app** — launch the Debug build. With the real data, confirm the flame shows on any account whose numbers satisfy the formula (compute one by hand from its card and the X button), and its tooltip reads per Task 6. Screenshot.

- [ ] **Step 8: Commit**

```bash
git add apps/macos/ClaudeDashboard/ViewModels apps/macos/ClaudeDashboard/Views/AccountCard.swift apps/macos/ClaudeDashboard/Views/DashboardPane.swift apps/macos/ClaudeDashboard/Views/MenuBarPopover.swift apps/macos/ClaudeDashboardTests/Critical/CriticalWiringTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): flame badge for critical accounts and feed auto switch"
```

### Task 9: InfoTip, settings tips and the Critical Accounts section

**Files:**
- Create: `apps/macos/ClaudeDashboard/Views/Components/InfoTip.swift`
- Create: `apps/macos/ClaudeDashboard/Views/Settings/SettingsTips.swift`
- Create: `apps/macos/ClaudeDashboard/Views/Settings/CriticalSettingsSection.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/Settings/GeneralSettingsPane.swift`
- Modify: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift` (one `@Published var helpFocus`)
- Modify: `apps/macos/ClaudeDashboard/Views/HelpView.swift` (focus binding + scroll), `apps/macos/ClaudeDashboard/Views/MainWindow.swift:73`
- Test: `apps/macos/ClaudeDashboardTests/Critical/SettingsTipsTests.swift`

**Interfaces:**
- Consumes: `HelpTopic` (adds `.criticalAccounts` here so tips can link to it; Task 10 writes its content), `CriticalSettings`.
- Produces: `struct SettingTip: Equatable { summary: String; detail: String; example: String; topic: HelpTopic }`; `enum SettingsTips { static let all: [SettingTip]; static let autoRefresh, refreshInterval, autoPing, autoSwitch, fiveHourThreshold, sevenDayThreshold, menuBarToggle, criticalPace, highCriticalPace, quietHours, timeZone, autoUpdate: SettingTip }`; `struct InfoTip: View { init(_ tip: SettingTip, onMoreHelp: (() -> Void)?) }`; `DashboardViewModel.helpFocus: HelpTopic?`; `HelpView(focus: Binding<HelpTopic?>)`.

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

final class SettingsTipsTests: XCTestCase {
    func testEveryTipHasAllParts() {
        XCTAssertEqual(SettingsTips.all.count, 12)
        for tip in SettingsTips.all {
            XCTAssertFalse(tip.summary.isEmpty)
            XCTAssertFalse(tip.detail.isEmpty, tip.summary)
            XCTAssertFalse(tip.example.isEmpty, tip.summary)
        }
    }

    func testCriticalTipsLinkToTheCriticalTopic() {
        for tip in [SettingsTips.criticalPace, .highCriticalPace, .quietHours, .timeZone] {
            XCTAssertEqual(tip.topic, .criticalAccounts, tip.summary)
        }
    }

    /// The pace example's arithmetic must hold: X 16, pace 50 -> 8 per window.
    func testCriticalPaceExampleNumbers() {
        XCTAssertTrue(SettingsTips.criticalPace.example.contains("60 + 48 = 108"))
        XCTAssertTrue(SettingsTips.criticalPace.example.contains("55 + 40 = 95"))
        XCTAssertEqual(CriticalPlanner.projected(sevenDay: 60, activeHours: 30, pace: 50, x: 16), 108, accuracy: 1e-9)
        XCTAssertEqual(CriticalPlanner.projected(sevenDay: 55, activeHours: 25, pace: 50, x: 16), 95, accuracy: 1e-9)
    }

    func testHelpHasTheCriticalTopic() {
        XCTAssertTrue(HelpTopic.allCases.contains(.criticalAccounts))
        XCTAssertEqual(HelpTopic.criticalAccounts.title, "Critical Accounts")
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/SettingsTipsTests 2>&1 | tail -20
```
Expected: `cannot find 'SettingsTips' in scope`.

- [ ] **Step 3: Add the Help topic case** (`HelpView.swift`, `enum HelpTopic`): add `criticalAccounts` after `autoSwitch` in the case list, and in each switch:
  - `title`: `case .criticalAccounts: return "Critical Accounts"`
  - `subtitle`: `case .criticalAccounts: return "Spend weekly quota before it expires unused."`
  - `icon`: `case .criticalAccounts: return "flame"`
  - `color`: `case .criticalAccounts: return .red`
  - `content(for:)`: `case .criticalAccounts: criticalAccounts`, with this body (next to `autoSwitch`):

```swift
    private var criticalAccounts: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpText("An account is **critical** when its weekly (7d) quota will expire unused even if you keep using it steadily. The app marks it with a flame and, with **Auto Switch** on, moves Claude Code to it as soon as its 5h window is free.")
            HelpSubheading("X: what a 5h window costs")
            HelpText("**X** (Dashboard header) is the share of the 7d quota one full 5h window uses, measured every hour from the last 3 days of usage across all accounts. Click it for its history. Until enough history exists it shows **16% (default)**.")
            HelpSubheading("The two levels")
            HelpRow("**Critical** (orange flame): using **Critical pace** (default 50%) of every 5h window from now to the weekly reset, skipping **Quiet hours** (default 00:00 to 07:00, Asia/Ho_Chi_Minh), the account still ends at or under 100% of 7d.") {
                HelpSymbol(name: "flame", color: .orange)
            }
            HelpRow("**High critical** (red flame): even **High critical pace** (default 80%) around the clock, quiet hours included, ends at or under 100%.") {
                HelpSymbol(name: "flame.fill", color: .red)
            }
            HelpCallout(.note, "Formula: 7d now + (usable hours left / 5) x pace x X. Usable hours leave out quiet hours for Critical only.")
            HelpSubheading("Who wins")
            HelpSteps(["**High critical**: takes Claude Code from any account that is not high critical, the home account (bolt) included.",
                       "**Home account**: comes back after its 5h reset, unless Claude Code is on a high critical account.",
                       "**Critical**: takes Claude Code from an ordinary account that is not the home account.",
                       "**Thresholds**: the usual 5h/7d switch."])
            HelpSubheading("Examples (X = 16%)")
            HelpText("**1.** 7d at 55%, 25 usable hours left (5 windows). Critical pace 50% costs 8% per window: 55 + 40 = **95**, so critical, 5% would go unused.")
            HelpText("**2.** 7d at 60%, reset in 48 hours from 10:00, quiet 00:00 to 07:00: 34 usable hours (6.8 windows) x 8% = 54.4, so 60 + 54.4 = **114.4**: not critical. At 7d 40% it is 94.4: critical.")
            HelpText("**3.** 7d at 60%, reset in 10 hours. High critical pace 80% costs 12.8% per window: 60 + 25.6 = **85.6**, so high critical. If Claude Code is on the home account, it moves here when this account's 5h window is free.")
            HelpCallout(.tip, "Change the paces, quiet hours and time zone in **Settings › General › Critical Accounts**. Each setting's ⓘ shows another example.")
        }
    }
```

- [ ] **Step 4: Write `SettingsTips.swift`**

```swift
import Foundation

/// One setting's explanation: `summary` is the hover tooltip; the click popover shows
/// `detail`, then `example`, then a link to `topic` in Help.
struct SettingTip: Equatable {
    let summary: String
    let detail: String
    let example: String
    let topic: HelpTopic
}

/// The text of every `InfoTip` in Settings > General. Keep it in step with the Help topics.
enum SettingsTips {
    static let autoRefresh = SettingTip(
        summary: "Fetch usage for every account on a timer.",
        detail: "When on, the app fetches usage for all accounts every interval. When off, usage only updates when you press Refresh (auto switch still polls the active account near a limit).",
        example: "With a 5 min interval, a card that shows 40% at 10:00 shows the new figure by 10:05.",
        topic: .refreshUpdates)
    static let refreshInterval = SettingTip(
        summary: "Minutes between automatic refreshes (1 to 60).",
        detail: "Shorter intervals keep cards fresher and let auto switch react sooner, at the cost of more requests to claude.ai.",
        example: "At 1 min, auto switch sees a 5h window reach 95% within a minute; at 15 min it can be 15 minutes late.",
        topic: .refreshUpdates)
    static let autoPing = SettingTip(
        summary: "Start idle usage windows automatically.",
        detail: "A window only starts counting at the first message. After each refresh, Auto Ping sends one short message from every active account whose 5h or 7d window is not running, so it resets sooner.",
        example: "An account idle since yesterday shows 5h with no reset time. Auto Ping starts it at 09:00, so it resets at 14:00 instead of five hours after you first use it.",
        topic: .ping)
    static let autoSwitch = SettingTip(
        summary: "Move Claude Code to another account before a limit blocks you.",
        detail: "When the account Claude Code uses reaches a threshold, Claude Code moves to the account whose weekly quota is closest to going unused. Order: high critical, then the home account (bolt) after its 5h reset, then critical, then the threshold switch. Only accounts already signed in to Claude Code are used.",
        example: "Claude Code is on A at 5h 95%. B has 5h 10% and 7d 40% with two days left, so Claude Code moves to B.",
        topic: .autoSwitch)
    static let fiveHourThreshold = SettingTip(
        summary: "5h usage at which auto switch moves Claude Code.",
        detail: "When the active account's 5h usage reaches this, auto switch picks another account.",
        example: "At 95%, an account at 5h 94% stays; at 95% Claude Code moves.",
        topic: .autoSwitch)
    static let sevenDayThreshold = SettingTip(
        summary: "7d usage at which auto switch moves Claude Code.",
        detail: "When the active account's weekly usage reaches this, auto switch picks another account, whatever its 5h usage.",
        example: "At 99%, an account at 5h 20% but 7d 99% still moves Claude Code away.",
        topic: .autoSwitch)
    static let menuBarToggle = SettingTip(
        summary: "Show the auto switch toggle in the menu bar popover near a limit.",
        detail: "When auto switch is off, the popover footer shows its toggle once the active account comes close to a threshold, so you can turn it on in time.",
        example: "Auto switch off, thresholds 95/99: at 5h 80% the toggle appears in the popover footer.",
        topic: .autoSwitch)
    static let criticalPace = SettingTip(
        summary: "Assumed use per 5h window, outside quiet hours, for the critical check.",
        detail: "An account is critical when, using this share of every 5h window from now until its weekly reset (quiet hours excluded), it still would not reach 100% of 7d. X (Dashboard header) converts 5h use into 7d use.",
        example: "X 16%, pace 50%: each window costs 8% of 7d. 7d at 60% with 30 active hours left (6 windows): 60 + 48 = 108, so not critical. 7d at 55% with 25 hours (5 windows): 55 + 40 = 95, so critical.",
        topic: .criticalAccounts)
    static let highCriticalPace = SettingTip(
        summary: "Assumed use per 5h window, around the clock, for the high critical check.",
        detail: "An account is high critical when even this heavier pace, with no quiet hours, would not reach 100% of 7d by the reset. High critical accounts win over the home account.",
        example: "X 16%, pace 80%: 12.8% of 7d per window. 7d at 60% with 10 hours left (2 windows): 60 + 25.6 = 85.6, so high critical.",
        topic: .criticalAccounts)
    static let quietHours = SettingTip(
        summary: "Daily hours not counted as usable for the critical check.",
        detail: "Hours between start and end each day are left out when counting the 5h windows still usable before a weekly reset. A start after the end crosses midnight. Equal start and end means no quiet hours. High critical ignores quiet hours.",
        example: "Quiet 00:00 to 07:00, reset in 48 hours from 10:00: two quiet blocks drop 14 hours, leaving 34 hours (6.8 windows).",
        topic: .criticalAccounts)
    static let timeZone = SettingTip(
        summary: "Time zone the quiet hours are read in.",
        detail: "Quiet hours follow this zone, not the Mac's. Useful when travelling or when your working day follows another zone.",
        example: "Zone Asia/Ho_Chi_Minh, quiet 00:00 to 07:00: on a Mac set to Paris time that is 19:00 to 02:00 in summer.",
        topic: .criticalAccounts)
    static let autoUpdate = SettingTip(
        summary: "Install new releases once a day.",
        detail: "Checks GitHub once a day and installs a newer version automatically. Check for Updates does the same on demand.",
        example: "A release published at noon is installed by the next daily check.",
        topic: .refreshUpdates)

    static let all: [SettingTip] = [
        autoRefresh, refreshInterval, autoPing, autoSwitch, fiveHourThreshold, sevenDayThreshold,
        menuBarToggle, criticalPace, highCriticalPace, quietHours, timeZone, autoUpdate,
    ]
}
```

- [ ] **Step 5: Write `InfoTip.swift`**

```swift
import SwiftUI

/// An info icon for a setting: hover shows `tip.summary`; click opens the detail, an
/// example and a "More in Help" link.
///
///     LabeledContent { stepper } label: {
///         HStack { Text("Switch at 5h usage"); InfoTip(SettingsTips.fiveHourThreshold) { openHelp(.autoSwitch) } }
///     }
struct InfoTip: View {
    let tip: SettingTip
    let onMoreHelp: (() -> Void)?
    @State private var isShowing = false

    init(_ tip: SettingTip, onMoreHelp: (() -> Void)? = nil) {
        self.tip = tip
        self.onMoreHelp = onMoreHelp
    }

    var body: some View {
        Button { isShowing.toggle() } label: {
            Image(systemName: "info.circle").foregroundStyle(.secondary)
        }
        .buttonStyle(.borderless)
        .help(tip.summary)
        .accessibilityLabel("About this setting")
        .popover(isPresented: $isShowing, arrowEdge: .trailing) {
            VStack(alignment: .leading, spacing: 10) {
                Text(tip.detail)
                Text("Example").font(.caption.bold()).foregroundStyle(.secondary)
                Text(tip.example).foregroundStyle(.secondary)
                if let onMoreHelp {
                    Button("More in Help") {
                        isShowing = false
                        onMoreHelp()
                    }
                    .buttonStyle(.link)
                }
            }
            .fixedSize(horizontal: false, vertical: true)
            .padding(14)
            .frame(width: 340, alignment: .leading)
        }
    }
}
```

- [ ] **Step 6: Help focus**

`DashboardViewModel.swift`, below `@Published var selection`:

```swift
    /// A Help topic to scroll to when the Help pane opens (set by "More in Help").
    @Published var helpFocus: HelpTopic?
```

`HelpView.swift`: add `@Binding var focus: HelpTopic?` as the first property, update the doc example to `HelpView(focus: $vm.helpFocus)`, and attach to the `ScrollView` inside `ScrollViewReader`:

```swift
                .onAppear { scrollToFocus(proxy) }
                .onChange(of: focus) { _ in scrollToFocus(proxy) }
```

with, in the struct:

```swift
    private func scrollToFocus(_ proxy: ScrollViewProxy) {
        guard let topic = focus else { return }
        DispatchQueue.main.async {
            withAnimation { proxy.scrollTo(topic, anchor: .top) }
            focus = nil
        }
    }
```

`MainWindow.swift:73`: `HelpView(focus: $viewModel.helpFocus)`.

- [ ] **Step 7: Write `CriticalSettingsSection.swift`**

```swift
import SwiftUI

/// Settings > General > Critical Accounts: both paces, quiet hours and their time zone.
///
///     CriticalSettingsSection(settings: vm.criticalSettings, openHelp: { vm.openHelp($0) })
struct CriticalSettingsSection: View {
    @ObservedObject var settings: CriticalSettings
    let openHelp: (HelpTopic) -> Void

    var body: some View {
        Section {
            LabeledContent {
                Stepper("\(settings.pace)%", value: $settings.pace,
                        in: CriticalSettings.paceRange, step: CriticalSettings.paceStep)
            } label: { label("Critical pace", SettingsTips.criticalPace) }
            LabeledContent {
                Stepper("\(settings.highPace)%", value: $settings.highPace,
                        in: settings.pace...CriticalSettings.paceRange.upperBound, step: CriticalSettings.paceStep)
            } label: { label("High critical pace", SettingsTips.highCriticalPace) }
            LabeledContent {
                HStack(spacing: 6) {
                    hourPicker("Start", $settings.quietStart)
                    Text("to")
                    hourPicker("End", $settings.quietEnd)
                }
            } label: { label("Quiet hours", SettingsTips.quietHours) }
            LabeledContent {
                Picker("Time zone", selection: $settings.timeZoneId) {
                    ForEach(TimeZone.knownTimeZoneIdentifiers.sorted(), id: \.self) { Text($0).tag($0) }
                }
                .labelsHidden()
                .frame(maxWidth: 240)
            } label: { label("Time zone", SettingsTips.timeZone) }
        } header: {
            Text("Critical Accounts")
        } footer: {
            Text("Marks accounts whose weekly quota will go unused, and lets auto switch move Claude Code to them when their 5h window is free.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private func label(_ title: String, _ tip: SettingTip) -> some View {
        HStack(spacing: 4) {
            Text(title)
            InfoTip(tip) { openHelp(tip.topic) }
        }
    }

    private func hourPicker(_ title: String, _ value: Binding<Int>) -> some View {
        Picker(title, selection: value) {
            ForEach(CriticalSettings.hourRange, id: \.self) { Text(String(format: "%02d:00", $0)).tag($0) }
        }
        .labelsHidden()
        .frame(width: 84)
    }
}
```

`TimeZone.knownTimeZoneIdentifiers` has ~450 entries; a plain `Picker` menu is acceptable on macOS (scrollable menu). The spec's "searchable" picker is dropped as YAGNI unless review asks for it; note this in the Task 11 spec update.

- [ ] **Step 8: Tips on every General setting** (`GeneralSettingsPane.swift`)

Add:

```swift
    private func openHelp(_ topic: HelpTopic) {
        viewModel.helpFocus = topic
        viewModel.selection = .help
    }

    private func tipLabel(_ title: String, _ tip: SettingTip) -> some View {
        HStack(spacing: 4) {
            Text(title)
            InfoTip(tip) { openHelp(tip.topic) }
        }
    }
```

Then change each control's label (titles unchanged):

- `Toggle("Enable auto refresh", isOn: ...)` -> `Toggle(isOn: $viewModel.autoRefreshEnabled) { tipLabel("Enable auto refresh", SettingsTips.autoRefresh) }`
- `LabeledContent("Interval") { ... }` -> `LabeledContent { ... } label: { tipLabel("Interval", SettingsTips.refreshInterval) }`
- Auto Ping toggle -> `Toggle(isOn: <same Binding>) { tipLabel("Ping when a usage window is not running", SettingsTips.autoPing) }`
- `Toggle("Enable auto switch", ...)` -> label `tipLabel("Enable auto switch", SettingsTips.autoSwitch)`
- `LabeledContent("Switch at 5h usage")` -> label `tipLabel("Switch at 5h usage", SettingsTips.fiveHourThreshold)`
- `LabeledContent("Switch at 7d usage")` -> label `tipLabel("Switch at 7d usage", SettingsTips.sevenDayThreshold)`
- `Toggle("Show toggle in menu bar when near limit", ...)` -> label `tipLabel("Show toggle in menu bar when near limit", SettingsTips.menuBarToggle)`
- `Toggle("Auto-update daily", ...)` -> label `tipLabel("Auto-update daily", SettingsTips.autoUpdate)`

Insert after the Auto Switch `Section`:

```swift
                CriticalSettingsSection(settings: viewModel.criticalSettings, openHelp: openHelp)
```

Update the struct's doc comment to list the Critical Accounts section and the info tips.

- [ ] **Step 9: Run tests and the full suite**

```bash
cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests 2>&1 | tail -30
```
Expected: `** TEST SUCCEEDED **`.

- [ ] **Step 10: Run check in the app** — Settings > General: every setting has an ⓘ; hover shows the summary; click shows detail, example and "More in Help"; the link opens Help scrolled to the topic; changing Critical pace to 85 lifts High critical pace to 85; quiet hours and time zone persist across relaunch. Screenshot.

- [ ] **Step 11: Commit**

```bash
git add apps/macos/ClaudeDashboard/Views apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift apps/macos/ClaudeDashboardTests/Critical/SettingsTipsTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): info tips on every setting and the Critical Accounts section"
```

### Task 10: Help content

**Files:**
- Modify: `apps/macos/ClaudeDashboard/Views/HelpView.swift` (`autoSwitch`, `readingCard`; `criticalAccounts` came in Task 9)

**Interfaces:**
- Consumes: Help blocks `HelpText`, `HelpRow`, `HelpSymbol`, `HelpCallout`, `HelpSubheading` (`Views/Components/HelpBlocks.swift`).
- Produces: nothing new.

- [ ] **Step 1: Check the `criticalAccounts` body** written in Task 9 Step 3 against the UI labels Task 9 shipped (section "Critical Accounts", "Critical pace", "High critical pace", "Quiet hours", "Time zone"); fix any wording that drifted.

- [ ] **Step 2: Update `autoSwitch`** — insert after the "**Where to**" row:

```swift
            HelpRow("**First, critical accounts**: a high critical account takes Claude Code from anywhere, the home account (bolt) comes back after its 5h reset, and a critical account takes Claude Code from an ordinary account. See **Critical Accounts**.") {
                HelpSymbol(name: "flame", color: .red)
            }
```

- [ ] **Step 3: Update `readingCard`** — read the current `readingCard` body first, then add next to the bolt / badge rows:

```swift
            HelpRow("**Flame**: orange when the account is critical, red when high critical: its weekly quota will go unused. Hover for the numbers.") {
                HelpSymbol(name: "flame.fill", color: .red)
            }
            HelpRow("**X** (Dashboard header): what a full 5h window costs of the 7d quota. Click for its history.") {
                HelpSymbol(name: "chart.xyaxis.line", color: .orange)
            }
```

- [ ] **Step 4: Verify the arithmetic in the examples** — run `SettingsTipsTests` and check by hand: example 2: 48h from 10:00 with 00:00 to 07:00 quiet loses 14 hours, 34 / 5 = 6.8, 6.8 x 8 = 54.4. Example 3: 10 / 5 = 2, 2 x 12.8 = 25.6.

```bash
cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/SettingsTipsTests 2>&1 | tail -10
```

- [ ] **Step 5: Run check** — open Tools > Help: the Critical Accounts card renders, the chip in the grid scrolls to it, Auto Switch and Reading a Card show the new rows. Screenshot.

- [ ] **Step 6: Commit**

```bash
git add apps/macos/ClaudeDashboard/Views/HelpView.swift
git commit -m "docs(macos): Help for critical accounts, X and the new switch order"
```

### Task 11: Docs and final verification

**Files:**
- Modify: `CLAUDE.md` (Services layer: add a **Critical** bullet after AutoSwitch)
- Modify: `docs/superpowers/specs/2026-10-16-critical-accounts-design.md` (time zone picker: plain menu, not searchable; `ratio_logs.t` INTEGER seconds; exhausted week `7d >= 99` is `.none`)

- [ ] **Step 1: CLAUDE.md bullet** (after the AutoSwitch bullet):

```markdown
- **Critical** (`Services/Critical/`) — `UsageRatioController` recomputes X (7d % per full 5h window,
  all accounts pooled, last 3 days of `usage_logs`, `resetsAt` jitter tolerated) at launch and hourly
  into `ratio_logs.db` (90 days; default 16 until 100 points of 5h use exist); the Dashboard header
  shows it with a history chart. `CriticalPlanner` (pure) marks an account **critical** when
  `7d + usable hours / 5 x pace x X <= 100` at its weekly reset (pace 50, quiet hours 00:00-07:00
  Asia/Ho_Chi_Minh excluded) and **high critical** at pace 80 around the clock; `CriticalSettings`
  holds all five values. Auto switch order: high critical > home (bolt) > critical > threshold tiers;
  a target needs a free 5h window (`resetsAt == nil` or `<= 2%`).
  Spec: `docs/superpowers/specs/2026-10-16-critical-accounts-design.md`.
```

- [ ] **Step 2: Spec amendments** — edit the three lines named above to match what was built.

- [ ] **Step 3: Full verification**

```bash
cd apps/macos && xcodegen generate \
  && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -3 \
  && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | tail -5 \
  && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardHelperTests test 2>&1 | tail -5
wc -l ClaudeDashboard/ViewModels/DashboardViewModel.swift ClaudeDashboard/Views/HelpView.swift
```
Expected: build succeeds, both suites `** TEST SUCCEEDED **`, both files under 1000 lines. Known flaky: `CommandRunnerTests` (memory note); re-run once before treating as real.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git add -f docs/superpowers/specs/2026-10-16-critical-accounts-design.md
git commit -m "docs: critical accounts in CLAUDE.md and spec amendments"
```
