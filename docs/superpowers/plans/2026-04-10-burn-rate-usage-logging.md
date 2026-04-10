# Burn Rate Animal Icons + Usage Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add token consumption speed tracking with animal emoji indicators on progress bars, persist usage data to SQLite, and provide drill-down + overview charts.

**Architecture:** Approach B — Separated layer. `UsageLogStore` (SQLite) handles persistence with smart compression. `BurnRateTracker` calculates speed and selects animals. Both are `actor`-based for thread safety. Chart views use Swift Charts (native, macOS 13+). `AccountDetailView` for per-account drill-down, `OverviewChartView` for aggregated multi-account view.

**Tech Stack:** Swift 5, SwiftUI, SQLite3, Swift Charts, Combine

**Spec:** `docs/superpowers/specs/2026-04-10-burn-rate-usage-logging-design.md`

---

## File Map

### New Files
| File | Responsibility |
|------|---------------|
| `ClaudeDashboard/Models/UsageLogModels.swift` | `UsageWindow` enum, `UsageLogEntry`, `ResetCycle`, `BurnRateResult`, `BurnRates` structs |
| `ClaudeDashboard/Services/UsageLogStore.swift` | SQLite DB: create/migrate schema, record with smart compression, query, cleanup |
| `ClaudeDashboard/Services/BurnRateTracker.swift` | Speed calculation, animal selection, delegates logging to UsageLogStore |
| `ClaudeDashboard/ViewModels/AccountDetailViewModel.swift` | Query logs for chart data, manage selected window/cycle |
| `ClaudeDashboard/Views/AccountDetailView.swift` | Drill-down chart per account with Swift Charts |
| `ClaudeDashboard/Views/OverviewChartView.swift` | Aggregated multi-account chart with total line |
| `ClaudeDashboardTests/UsageLogStoreTests.swift` | Tests for SQLite operations + smart compression |
| `ClaudeDashboardTests/BurnRateTrackerTests.swift` | Tests for speed calculation + animal mapping |

### Modified Files
| File | Changes |
|------|---------|
| `ClaudeDashboard/ViewModels/DashboardViewModel.swift` | Add `burnRateTracker` property, extend `AccountUsageState` with `burnRates`, call tracker in `refreshAll()`, add navigation state |
| `ClaudeDashboard/Views/UsageBar.swift` | Add `animal: String?` parameter, render emoji above bar |
| `ClaudeDashboard/Views/AccountCard.swift` | Pass `burnRates` animal to `UsageBar`, add tap gesture for drill-down |
| `ClaudeDashboard/Views/DashboardWindow.swift` | Add "Overview" toolbar button, navigation to detail/overview views |
| `ClaudeDashboard/Views/MenuBarPopover.swift` | Add tap gesture on AccountCard for drill-down |
| `project.yml` | Add `Charts` framework dependency |

---

### Task 1: Models — UsageWindow, UsageLogEntry, ResetCycle, BurnRateResult

**Files:**
- Create: `ClaudeDashboard/Models/UsageLogModels.swift`
- Test: `ClaudeDashboardTests/UsageLogModelsTests.swift`

- [ ] **Step 1: Create the models file**

```swift
// ClaudeDashboard/Models/UsageLogModels.swift
import Foundation

enum UsageWindow: Int, CaseIterable {
    case fiveHour = 0
    case sevenDay = 1
    case sonnet = 2

    var label: String {
        switch self {
        case .fiveHour: return "5h"
        case .sevenDay: return "7d"
        case .sonnet: return "S"
        }
    }
}

struct UsageLogEntry: Identifiable, Equatable {
    let id: Int64
    let accountId: UUID
    let window: UsageWindow
    let resetsAt: Date
    let recordedAt: Date
    let utilization: Double    // 0-100
    let isLimited: Bool
}

struct ResetCycle: Identifiable, Equatable {
    var id: Date { resetsAt }
    let resetsAt: Date
    let firstRecordedAt: Date
    let lastRecordedAt: Date
    let peakUtilization: Double
    let dataPointCount: Int
}

struct BurnRateResult: Equatable {
    let level: Int              // 1-5
    let animal: String          // emoji
    let projectedTime: TimeInterval  // seconds until 100%

    static let animals: [Int: String] = [
        1: "🐌",  // > 5h
        2: "🐢",  // 3-5h
        3: "🐇",  // 1.5-3h
        4: "🐎",  // 0.5-1.5h
        5: "🐆",  // < 30m
    ]

    static func fromProjectedTime(_ seconds: TimeInterval) -> BurnRateResult {
        let hours = seconds / 3600
        let level: Int
        if hours > 5 { level = 1 }
        else if hours > 3 { level = 2 }
        else if hours > 1.5 { level = 3 }
        else if hours > 0.5 { level = 4 }
        else { level = 5 }
        return BurnRateResult(level: level, animal: animals[level]!, projectedTime: seconds)
    }
}

struct BurnRates: Equatable {
    var fiveHour: BurnRateResult?
    var sevenDay: BurnRateResult?
    var sonnet: BurnRateResult?
}
```

- [ ] **Step 2: Write tests for BurnRateResult.fromProjectedTime**

```swift
// ClaudeDashboardTests/UsageLogModelsTests.swift
import XCTest
@testable import ClaudeDashboard

final class UsageLogModelsTests: XCTestCase {
    func testBurnRateLevel1_over5hours() {
        let result = BurnRateResult.fromProjectedTime(6 * 3600) // 6 hours
        XCTAssertEqual(result.level, 1)
        XCTAssertEqual(result.animal, "🐌")
    }

    func testBurnRateLevel2_3to5hours() {
        let result = BurnRateResult.fromProjectedTime(4 * 3600) // 4 hours
        XCTAssertEqual(result.level, 2)
        XCTAssertEqual(result.animal, "🐢")
    }

    func testBurnRateLevel3_1point5to3hours() {
        let result = BurnRateResult.fromProjectedTime(2 * 3600) // 2 hours
        XCTAssertEqual(result.level, 3)
        XCTAssertEqual(result.animal, "🐇")
    }

    func testBurnRateLevel4_30mto1point5hours() {
        let result = BurnRateResult.fromProjectedTime(1 * 3600) // 1 hour
        XCTAssertEqual(result.level, 4)
        XCTAssertEqual(result.animal, "🐎")
    }

    func testBurnRateLevel5_under30min() {
        let result = BurnRateResult.fromProjectedTime(15 * 60) // 15 min
        XCTAssertEqual(result.level, 5)
        XCTAssertEqual(result.animal, "🐆")
    }

    func testBurnRateBoundary_exactly5hours() {
        let result = BurnRateResult.fromProjectedTime(5 * 3600)
        XCTAssertEqual(result.level, 2) // 5h is not > 5h, so level 2
    }

    func testBurnRateBoundary_exactly30min() {
        let result = BurnRateResult.fromProjectedTime(30 * 60)
        XCTAssertEqual(result.level, 5) // 0.5h is not > 0.5h, so level 5
    }

    func testUsageWindowLabels() {
        XCTAssertEqual(UsageWindow.fiveHour.label, "5h")
        XCTAssertEqual(UsageWindow.sevenDay.label, "7d")
        XCTAssertEqual(UsageWindow.sonnet.label, "S")
    }

    func testUsageWindowRawValues() {
        XCTAssertEqual(UsageWindow.fiveHour.rawValue, 0)
        XCTAssertEqual(UsageWindow.sevenDay.rawValue, 1)
        XCTAssertEqual(UsageWindow.sonnet.rawValue, 2)
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageLogModelsTests 2>&1 | tail -20`
Expected: All tests PASS

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/Models/UsageLogModels.swift ClaudeDashboardTests/UsageLogModelsTests.swift
git commit -m "feat: add UsageWindow, UsageLogEntry, ResetCycle, BurnRateResult models"
```

---

### Task 2: UsageLogStore — SQLite Setup + Schema

**Files:**
- Create: `ClaudeDashboard/Services/UsageLogStore.swift`
- Test: `ClaudeDashboardTests/UsageLogStoreTests.swift`

- [ ] **Step 1: Write the failing test for DB initialization**

```swift
// ClaudeDashboardTests/UsageLogStoreTests.swift
import XCTest
@testable import ClaudeDashboard

final class UsageLogStoreTests: XCTestCase {
    var store: UsageLogStore!
    var dbPath: String!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "test_usage_\(UUID().uuidString).db"
        store = await UsageLogStore(dbPath: dbPath)
    }

    override func tearDown() async throws {
        store = nil
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    func testDatabaseCreated() async {
        XCTAssertTrue(FileManager.default.fileExists(atPath: dbPath))
    }

    func testRecordAndQuerySingleEntry() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 45.5, isLimited: false)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 1)
        XCTAssertEqual(logs[0].accountId, accountId)
        XCTAssertEqual(logs[0].window, .fiveHour)
        XCTAssertEqual(logs[0].utilization, 45.5, accuracy: 0.01)
        XCTAssertFalse(logs[0].isLimited)
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageLogStoreTests 2>&1 | tail -20`
Expected: FAIL — `UsageLogStore` not found

- [ ] **Step 3: Implement UsageLogStore with schema creation and basic record/query**

```swift
// ClaudeDashboard/Services/UsageLogStore.swift
import Foundation
import SQLite3

actor UsageLogStore {
    private var db: OpaquePointer?

    init(dbPath: String? = nil) {
        let path = dbPath ?? UsageLogStore.defaultDBPath()
        openDatabase(at: path)
        createTables()
    }

    deinit {
        if let db { sqlite3_close(db) }
    }

    // MARK: - Record

    func record(accountId: UUID, window: UsageWindow, resetsAt: Date, utilization: Double, isLimited: Bool) {
        let aid = resolveAccountId(accountId)
        let w = Int32(window.rawValue)
        let rat = Int64(resetsAt.timeIntervalSince1970)
        let t = Int64(Date().timeIntervalSince1970)
        let u = Int64(round(utilization * 100))
        let lim: Int32 = isLimited ? 1 : 0

        applyCompression(aid: aid, w: w, rat: rat, u: u)

        let sql = "INSERT INTO usage_logs (aid, w, rat, t, u, lim) VALUES (?, ?, ?, ?, ?, ?)"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int64(stmt, 1, Int64(aid))
        sqlite3_bind_int(stmt, 2, w)
        sqlite3_bind_int64(stmt, 3, rat)
        sqlite3_bind_int64(stmt, 4, t)
        sqlite3_bind_int64(stmt, 5, u)
        sqlite3_bind_int(stmt, 6, lim)
        sqlite3_step(stmt)
    }

    // MARK: - Query

    func logs(accountId: UUID, window: UsageWindow, from: Date?, to: Date?) -> [UsageLogEntry] {
        guard let aid = lookupAccountId(accountId) else { return [] }
        var sql = "SELECT id, t, u, lim, rat FROM usage_logs WHERE aid = ? AND w = ?"
        var params: [Any] = [Int64(aid), Int32(window.rawValue)]

        if let from {
            sql += " AND t >= ?"
            params.append(Int64(from.timeIntervalSince1970))
        }
        if let to {
            sql += " AND t <= ?"
            params.append(Int64(to.timeIntervalSince1970))
        }
        sql += " ORDER BY t ASC"

        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int64(stmt, 1, params[0] as! Int64)
        sqlite3_bind_int(stmt, 2, params[1] as! Int32)
        if params.count > 2 { sqlite3_bind_int64(stmt, 3, params[2] as! Int64) }
        if params.count > 3 { sqlite3_bind_int64(stmt, 4, params[3] as! Int64) }

        var results: [UsageLogEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            let entry = UsageLogEntry(
                id: sqlite3_column_int64(stmt, 0),
                accountId: accountId,
                window: window,
                resetsAt: Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 4))),
                recordedAt: Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 1))),
                utilization: Double(sqlite3_column_int64(stmt, 2)) / 100.0,
                isLimited: sqlite3_column_int(stmt, 3) != 0
            )
            results.append(entry)
        }
        return results
    }

    func allLogs(window: UsageWindow, from: Date?, to: Date?) -> [UsageLogEntry] {
        var sql = "SELECT l.id, l.t, l.u, l.lim, l.rat, a.account_id FROM usage_logs l JOIN accounts_map a ON l.aid = a.aid WHERE l.w = ?"
        var binds: [Int64] = [Int64(window.rawValue)]

        if let from {
            sql += " AND l.t >= ?"
            binds.append(Int64(from.timeIntervalSince1970))
        }
        if let to {
            sql += " AND l.t <= ?"
            binds.append(Int64(to.timeIntervalSince1970))
        }
        sql += " ORDER BY l.t ASC"

        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        for (i, val) in binds.enumerated() {
            sqlite3_bind_int64(stmt, Int32(i + 1), val)
        }

        var results: [UsageLogEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            guard let uuidStr = sqlite3_column_text(stmt, 5),
                  let uuid = UUID(uuidString: String(cString: uuidStr)) else { continue }
            let entry = UsageLogEntry(
                id: sqlite3_column_int64(stmt, 0),
                accountId: uuid,
                window: window,
                resetsAt: Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 4))),
                recordedAt: Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 1))),
                utilization: Double(sqlite3_column_int64(stmt, 2)) / 100.0,
                isLimited: sqlite3_column_int(stmt, 3) != 0
            )
            results.append(entry)
        }
        return results
    }

    func resetCycles(accountId: UUID, window: UsageWindow) -> [ResetCycle] {
        guard let aid = lookupAccountId(accountId) else { return [] }
        let sql = """
            SELECT rat, MIN(t), MAX(t), MAX(u), COUNT(*)
            FROM usage_logs WHERE aid = ? AND w = ?
            GROUP BY rat ORDER BY rat DESC
        """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int64(stmt, 1, Int64(aid))
        sqlite3_bind_int(stmt, 2, Int32(window.rawValue))

        var results: [ResetCycle] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            results.append(ResetCycle(
                resetsAt: Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 0))),
                firstRecordedAt: Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 1))),
                lastRecordedAt: Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 2))),
                peakUtilization: Double(sqlite3_column_int64(stmt, 3)) / 100.0,
                dataPointCount: Int(sqlite3_column_int(stmt, 4))
            ))
        }
        return results
    }

    func deleteOlderThan(_ date: Date) {
        let timestamp = Int64(date.timeIntervalSince1970)
        let sql = "DELETE FROM usage_logs WHERE t < ?"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_int64(stmt, 1, timestamp)
        sqlite3_step(stmt)
    }

    // MARK: - Smart Compression

    private func applyCompression(aid: Int32, w: Int32, rat: Int64, u: Int64) {
        let sql = "SELECT id, u FROM usage_logs WHERE aid = ? AND w = ? AND rat = ? ORDER BY t DESC LIMIT 2"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int64(stmt, 1, Int64(aid))
        sqlite3_bind_int(stmt, 2, w)
        sqlite3_bind_int64(stmt, 3, rat)

        var recent: [(id: Int64, u: Int64)] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            recent.append((id: sqlite3_column_int64(stmt, 0), u: sqlite3_column_int64(stmt, 1)))
        }

        // If 2 most recent have same value AND new value is also the same → delete the middle one
        guard recent.count >= 2,
              recent[0].u == recent[1].u,
              recent[0].u == u else { return }

        let deleteSQL = "DELETE FROM usage_logs WHERE id = ?"
        var deleteStmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, deleteSQL, -1, &deleteStmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(deleteStmt) }
        sqlite3_bind_int64(deleteStmt, 1, recent[0].id) // delete most recent (middle)
        sqlite3_step(deleteStmt)
    }

    // MARK: - Account ID Mapping

    private func resolveAccountId(_ uuid: UUID) -> Int32 {
        if let existing = lookupAccountId(uuid) { return existing }
        let sql = "INSERT INTO accounts_map (account_id) VALUES (?)"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return 0 }
        defer { sqlite3_finalize(stmt) }
        let str = uuid.uuidString
        sqlite3_bind_text(stmt, 1, (str as NSString).utf8String, -1, nil)
        sqlite3_step(stmt)
        return Int32(sqlite3_last_insert_rowid(db))
    }

    private func lookupAccountId(_ uuid: UUID) -> Int32? {
        let sql = "SELECT aid FROM accounts_map WHERE account_id = ?"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_finalize(stmt) }
        let str = uuid.uuidString
        sqlite3_bind_text(stmt, 1, (str as NSString).utf8String, -1, nil)
        guard sqlite3_step(stmt) == SQLITE_ROW else { return nil }
        return sqlite3_column_int(stmt, 0)
    }

    // MARK: - Database Setup

    private func openDatabase(at path: String) {
        let dir = (path as NSString).deletingLastPathComponent
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        guard sqlite3_open(path, &db) == SQLITE_OK else {
            print("[UsageLogStore] Failed to open database at \(path)")
            return
        }
    }

    private func createTables() {
        let sqls = [
            """
            CREATE TABLE IF NOT EXISTS accounts_map (
                aid INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT NOT NULL UNIQUE
            )
            """,
            """
            CREATE TABLE IF NOT EXISTS usage_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                aid INTEGER NOT NULL,
                w INTEGER NOT NULL,
                rat INTEGER NOT NULL,
                t INTEGER NOT NULL,
                u INTEGER NOT NULL,
                lim INTEGER DEFAULT 0
            )
            """,
            "CREATE INDEX IF NOT EXISTS idx_logs_lookup ON usage_logs(aid, w, rat, t)"
        ]
        for sql in sqls {
            sqlite3_exec(db, sql, nil, nil, nil)
        }
    }

    static func defaultDBPath() -> String {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let dir = appSupport.appendingPathComponent("ClaudeDashboard")
        return dir.appendingPathComponent("usage_logs.db").path
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageLogStoreTests 2>&1 | tail -20`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add ClaudeDashboard/Services/UsageLogStore.swift ClaudeDashboardTests/UsageLogStoreTests.swift
git commit -m "feat: add UsageLogStore with SQLite schema, record, query, compression"
```

---

### Task 3: UsageLogStore — Smart Compression Tests

**Files:**
- Modify: `ClaudeDashboardTests/UsageLogStoreTests.swift`

- [ ] **Step 1: Write compression tests**

Add to `UsageLogStoreTests.swift`:

```swift
    func testSmartCompression_threeIdenticalValues_keepFirstAndLast() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        // Record 3 identical values
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000) // 10ms gap for distinct timestamps
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 2, "Should keep first and last of identical streak")
        XCTAssertEqual(logs[0].utilization, 50.0, accuracy: 0.01)
        XCTAssertEqual(logs[1].utilization, 50.0, accuracy: 0.01)
        // First should be earlier than last
        XCTAssertTrue(logs[0].recordedAt < logs[1].recordedAt)
    }

    func testSmartCompression_valueChanges_noCompression() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 30.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 70.0, isLimited: false)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 3, "All 3 should remain — values differ")
    }

    func testSmartCompression_fourIdenticalValues_keepFirstAndLast() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        for _ in 0..<4 {
            await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 60.0, isLimited: false)
            try? await Task.sleep(nanoseconds: 10_000_000)
        }

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 2, "Should keep only first and last of 4 identical values")
    }

    func testSmartCompression_plateauThenChange() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        // 3 identical → should compress to 2
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 40.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 40.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 40.0, isLimited: false)
        try? await Task.sleep(nanoseconds: 10_000_000)
        // Then a change
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 60.0, isLimited: false)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 3, "2 from plateau + 1 new value")
        XCTAssertEqual(logs[0].utilization, 40.0, accuracy: 0.01)
        XCTAssertEqual(logs[1].utilization, 40.0, accuracy: 0.01)
        XCTAssertEqual(logs[2].utilization, 60.0, accuracy: 0.01)
    }

    func testDifferentWindowsNotCompressed() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)
        await store.record(accountId: accountId, window: .sevenDay, resetsAt: resetsAt, utilization: 50.0, isLimited: false)

        let logs5h = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        let logs7d = await store.logs(accountId: accountId, window: .sevenDay, from: nil, to: nil)
        XCTAssertEqual(logs5h.count, 1)
        XCTAssertEqual(logs7d.count, 1)
    }

    func testResetCycles() async {
        let accountId = UUID()
        let reset1 = Date().addingTimeInterval(3600)
        let reset2 = Date().addingTimeInterval(7200)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: reset1, utilization: 30.0, isLimited: false)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: reset1, utilization: 60.0, isLimited: false)
        await store.record(accountId: accountId, window: .fiveHour, resetsAt: reset2, utilization: 10.0, isLimited: false)

        let cycles = await store.resetCycles(accountId: accountId, window: .fiveHour)
        XCTAssertEqual(cycles.count, 2)
        // Ordered DESC by resetsAt
        XCTAssertEqual(cycles[0].peakUtilization, 10.0, accuracy: 0.01)
        XCTAssertEqual(cycles[0].dataPointCount, 1)
        XCTAssertEqual(cycles[1].peakUtilization, 60.0, accuracy: 0.01)
        XCTAssertEqual(cycles[1].dataPointCount, 2)
    }

    func testDeleteOlderThan() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 50.0, isLimited: false)

        // Delete everything older than 1 hour in the future (i.e., everything)
        await store.deleteOlderThan(Date().addingTimeInterval(3600))

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 0)
    }

    func testIsLimitedFlag() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        await store.record(accountId: accountId, window: .fiveHour, resetsAt: resetsAt, utilization: 100.0, isLimited: true)

        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        XCTAssertEqual(logs.count, 1)
        XCTAssertTrue(logs[0].isLimited)
    }
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageLogStoreTests 2>&1 | tail -20`
Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboardTests/UsageLogStoreTests.swift
git commit -m "test: add comprehensive UsageLogStore tests for compression, cycles, cleanup"
```

---

### Task 4: BurnRateTracker — Speed Calculation

**Files:**
- Create: `ClaudeDashboard/Services/BurnRateTracker.swift`
- Create: `ClaudeDashboardTests/BurnRateTrackerTests.swift`

- [ ] **Step 1: Write failing tests**

```swift
// ClaudeDashboardTests/BurnRateTrackerTests.swift
import XCTest
@testable import ClaudeDashboard

final class BurnRateTrackerTests: XCTestCase {
    var tracker: BurnRateTracker!
    var store: UsageLogStore!
    var dbPath: String!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "test_tracker_\(UUID().uuidString).db"
        store = await UsageLogStore(dbPath: dbPath)
        tracker = await BurnRateTracker(logStore: store)
    }

    override func tearDown() async throws {
        tracker = nil
        store = nil
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    func testFirstMeasurement_returnsNil() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)

        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 30.0, resetsAt: resetsAt
        )
        XCTAssertNil(result, "First measurement cannot compute speed")
    }

    func testSecondMeasurement_returnsRate() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 20.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 40.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(600) // 10 min later
        )

        XCTAssertNotNil(result)
        // Rate: 20% in 600s → 0.0333%/s → time to 100% from 40% = 60%/0.0333 = 1800s = 30min
        XCTAssertEqual(result!.level, 5) // < 30min → 🐆
        XCTAssertEqual(result!.animal, "🐆")
    }

    func testDifferentResetCycle_resetsHistory() async {
        let accountId = UUID()
        let reset1 = Date().addingTimeInterval(3600)
        let reset2 = Date().addingTimeInterval(7200) // different cycle
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 30.0, resetsAt: reset1, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 10.0, resetsAt: reset2, recordedAt: now.addingTimeInterval(300)
        )
        XCTAssertNil(result, "New cycle → reset history → nil")
    }

    func testUnchangedUtilization_gapOver5min_returnsNil() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 50.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 50.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(600) // 10 min
        )
        XCTAssertNil(result, "Unchanged + gap >= 5min → nil")
    }

    func testUnchangedUtilization_gapUnder5min_withPrev_keepsPrevRate() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(18000)
        let now = Date()

        // Measurement 1: 20%
        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 20.0, resetsAt: resetsAt, recordedAt: now
        )
        // Measurement 2: 40% (10 min later) → rate established
        let result2 = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 40.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(600)
        )
        XCTAssertNotNil(result2)

        // Measurement 3: 40% (2 min later, < 5 min gap) → keep prev rate
        let result3 = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 40.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(720)
        )
        XCTAssertNotNil(result3, "Gap < 5min + has prev → keep rate")
        XCTAssertEqual(result3!.level, result2!.level)
    }

    func testUnchangedUtilization_gapUnder5min_noPrev_returnsNil() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        // Only one measurement then immediate repeat
        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 50.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 50.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(120) // 2 min
        )
        XCTAssertNil(result, "Gap < 5min but no prev → nil")
    }

    func testUtilizationDecreases_resetsHistory() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 60.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 30.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(300)
        )
        XCTAssertNil(result, "Utilization decreased → reset → nil")
    }

    func testUtilization100_isLimitedLogged() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(3600)
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 80.0, resetsAt: resetsAt, recordedAt: now
        )
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 100.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(300)
        )
        XCTAssertNotNil(result)
        XCTAssertEqual(result!.level, 5) // 🐆

        // Verify log has isLimited = true
        let logs = await store.logs(accountId: accountId, window: .fiveHour, from: nil, to: nil)
        let limitedLogs = logs.filter { $0.isLimited }
        XCTAssertFalse(limitedLogs.isEmpty, "100% utilization should be logged as limited")
    }

    func testSlowRate_level1() async {
        let accountId = UUID()
        let resetsAt = Date().addingTimeInterval(18000) // 5h away
        let now = Date()

        let _ = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 5.0, resetsAt: resetsAt, recordedAt: now
        )
        // 5% in 30 min → rate = 0.00278%/s → time to 100% from 10% = 90%/0.00278 = 32400s = 9h
        let result = await tracker.record(
            accountId: accountId, window: .fiveHour,
            utilization: 10.0, resetsAt: resetsAt, recordedAt: now.addingTimeInterval(1800)
        )
        XCTAssertNotNil(result)
        XCTAssertEqual(result!.level, 1) // 🐌 > 5h
        XCTAssertEqual(result!.animal, "🐌")
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/BurnRateTrackerTests 2>&1 | tail -20`
Expected: FAIL — `BurnRateTracker` not found

- [ ] **Step 3: Implement BurnRateTracker**

```swift
// ClaudeDashboard/Services/BurnRateTracker.swift
import Foundation

actor BurnRateTracker {
    let logStore: UsageLogStore

    private struct Measurement {
        let utilization: Double
        let recordedAt: Date
        let resetsAt: Date
    }

    private struct HistoryEntry {
        var prev: Measurement?
        var current: Measurement?
        var lastRate: Double?  // %/second
    }

    private var history: [String: HistoryEntry] = [:]

    init(logStore: UsageLogStore) {
        self.logStore = logStore
    }

    func record(
        accountId: UUID,
        window: UsageWindow,
        utilization: Double,
        resetsAt: Date,
        recordedAt: Date = Date()
    ) async -> BurnRateResult? {
        let key = "\(accountId.uuidString)_\(window.rawValue)"
        let isLimited = utilization >= 100.0

        // Log to store (cross-actor call requires await)
        await logStore.record(
            accountId: accountId, window: window, resetsAt: resetsAt,
            utilization: utilization, isLimited: isLimited
        )

        let newMeasurement = Measurement(
            utilization: utilization, recordedAt: recordedAt, resetsAt: resetsAt
        )

        guard var entry = history[key], let current = entry.current else {
            // First measurement
            history[key] = HistoryEntry(prev: nil, current: newMeasurement, lastRate: nil)
            return nil
        }

        // Different reset cycle → reset
        guard resetsAt == current.resetsAt else {
            history[key] = HistoryEntry(prev: nil, current: newMeasurement, lastRate: nil)
            return nil
        }

        // Utilization decreased → reset (anomaly or post-reset)
        guard utilization >= current.utilization else {
            history[key] = HistoryEntry(prev: nil, current: newMeasurement, lastRate: nil)
            return nil
        }

        // Utilization changed
        if utilization > current.utilization {
            let deltaPercent = utilization - current.utilization
            let deltaTime = recordedAt.timeIntervalSince(current.recordedAt)
            guard deltaTime > 0 else { return nil }

            let rate = deltaPercent / deltaTime  // %/second
            let remaining = 100.0 - utilization
            let projectedTime = remaining / rate  // seconds

            entry.prev = current
            entry.current = newMeasurement
            entry.lastRate = rate
            history[key] = entry

            return BurnRateResult.fromProjectedTime(projectedTime)
        }

        // Utilization unchanged
        let gap = recordedAt.timeIntervalSince(current.recordedAt)

        if gap >= 300 { // >= 5 minutes
            // Update current to new timestamp (for compression bookkeeping)
            entry.current = newMeasurement
            entry.lastRate = nil
            history[key] = entry
            return nil
        }

        // Gap < 5 minutes — keep previous rate if available
        guard let lastRate = entry.lastRate, entry.prev != nil else {
            entry.current = newMeasurement
            history[key] = entry
            return nil
        }

        let remaining = 100.0 - utilization
        guard remaining > 0 else {
            return BurnRateResult.fromProjectedTime(0)
        }
        let projectedTime = remaining / lastRate
        entry.current = newMeasurement
        history[key] = entry
        return BurnRateResult.fromProjectedTime(projectedTime)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/BurnRateTrackerTests 2>&1 | tail -20`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add ClaudeDashboard/Services/BurnRateTracker.swift ClaudeDashboardTests/BurnRateTrackerTests.swift
git commit -m "feat: add BurnRateTracker with speed calculation and animal mapping"
```

---

### Task 5: Integrate Tracker into DashboardViewModel

**Files:**
- Modify: `ClaudeDashboard/ViewModels/DashboardViewModel.swift`

- [ ] **Step 1: Add burnRates to AccountUsageState**

In `DashboardViewModel.swift`, modify the `AccountUsageState` struct (line 5-11):

```swift
struct AccountUsageState: Identifiable {
    let id: UUID
    var account: Account
    var usage: UsageData?
    var isLoading: Bool = false
    var error: String?
    var burnRates: BurnRates?
}
```

- [ ] **Step 2: Add tracker property and inject it**

In `DashboardViewModel` class, add property after `apiService` (around line 27):

```swift
    private let burnRateTracker: BurnRateTracker
```

Modify `init` (line 30) to accept and create tracker:

```swift
    init(accountStore: AccountStore = AccountStore(), apiService: UsageAPIService = UsageAPIService(), logStore: UsageLogStore? = nil) {
        self.autoRefreshEnabled = UserDefaults.standard.object(forKey: "autoRefreshEnabled") as? Bool ?? true
        self.autoRefreshMinutes = {
            let val = UserDefaults.standard.integer(forKey: "autoRefreshMinutes")
            return val > 0 ? val : 5
        }()
        self.accountStore = accountStore
        self.apiService = apiService

        let store = logStore ?? UsageLogStore()
        self.logStore = store
        self.burnRateTracker = BurnRateTracker(logStore: store)

        // Cleanup old logs on launch
        Task {
            await store.deleteOlderThan(Date().addingTimeInterval(-90 * 24 * 3600))
        }

        accountStore.$accounts
            .receive(on: DispatchQueue.main)
            .sink { [weak self] accounts in
                self?.syncStates(with: accounts)
            }
            .store(in: &cancellables)

        scheduleAutoRefresh()
        Task { await self.refreshAll() }
    }
```

- [ ] **Step 3: Call tracker in refreshAll() after each fetch**

In `refreshAll()`, after usage is assigned (around line 97-112), add burn rate recording. Replace the `for await` block:

```swift
            for await (accountId, usage, error, planHint) in group {
                if let index = accountStates.firstIndex(where: { $0.id == accountId }) {
                    accountStates[index].usage = usage ?? accountStates[index].usage
                    accountStates[index].error = error

                    if error == "expired" {
                        var account = accountStates[index].account
                        account.status = .expired
                        accountStore.updateAccount(account)
                    } else if error == nil {
                        var account = accountStates[index].account
                        account.status = .active
                        account.lastSynced = Date()
                        if let planHint, account.plan != planHint {
                            account.plan = planHint
                        }
                        accountStore.updateAccount(account)
                    }

                    // Record burn rates
                    if let currentUsage = accountStates[index].usage {
                        var rates = BurnRates()
                        rates.fiveHour = await burnRateTracker.record(
                            accountId: accountId, window: .fiveHour,
                            utilization: currentUsage.fiveHour.utilization,
                            resetsAt: currentUsage.fiveHour.resetsAt ?? Date().addingTimeInterval(18000)
                        )
                        rates.sevenDay = await burnRateTracker.record(
                            accountId: accountId, window: .sevenDay,
                            utilization: currentUsage.sevenDay.utilization,
                            resetsAt: currentUsage.sevenDay.resetsAt ?? Date().addingTimeInterval(604800)
                        )
                        if let sonnet = currentUsage.sevenDaySonnet {
                            rates.sonnet = await burnRateTracker.record(
                                accountId: accountId, window: .sonnet,
                                utilization: sonnet.utilization,
                                resetsAt: sonnet.resetsAt ?? Date().addingTimeInterval(604800)
                            )
                        }
                        accountStates[index].burnRates = rates
                    }
                }
            }
```

- [ ] **Step 4: Store logStore directly on ViewModel**

The `logStore` is already created in init. Store it as a direct property so views can query it without crossing the tracker actor boundary. Add after `apiService` property:

```swift
    let logStore: UsageLogStore
```

And in init, add `self.logStore = store` right before creating the tracker:

```swift
        let store = logStore ?? UsageLogStore()
        self.logStore = store
        self.burnRateTracker = BurnRateTracker(logStore: store)
```

Note: `BurnRateTracker.logStore` is already `let` (public by default in actor) from the implementation in Task 4.

- [ ] **Step 5: Build to verify compilation**

Run: `xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`
Expected: BUILD SUCCEEDED

- [ ] **Step 6: Commit**

```bash
git add ClaudeDashboard/ViewModels/DashboardViewModel.swift ClaudeDashboard/Services/BurnRateTracker.swift
git commit -m "feat: integrate BurnRateTracker into DashboardViewModel refresh flow"
```

---

### Task 6: UsageBar — Animal Emoji Overlay

**Files:**
- Modify: `ClaudeDashboard/Views/UsageBar.swift`

- [ ] **Step 1: Add animal parameter**

Add parameter and update init (lines 4-14):

```swift
struct UsageBar: View {
    let label: String
    let utilization: Double
    let resetsAt: Date?
    let totalSeconds: TimeInterval
    let animal: String?

    init(label: String, utilization: Double, resetsAt: Date?, totalSeconds: TimeInterval = 18000, animal: String? = nil) {
        self.label = label
        self.utilization = utilization
        self.resetsAt = resetsAt
        self.totalSeconds = totalSeconds
        self.animal = animal
    }
```

- [ ] **Step 2: Add animal overlay to bar**

Replace the GeometryReader block (lines 24-33) to add animal above the bar:

```swift
                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.primary.opacity(0.1))

                        RoundedRectangle(cornerRadius: 4)
                            .fill(DashboardViewModel.usageColor(for: utilization))
                            .frame(width: geo.size.width * min(utilization / 100, 1.0))

                        if let animal {
                            Text(animal)
                                .font(.system(size: 12))
                                .offset(
                                    x: geo.size.width * min(utilization / 100, 1.0) - 8,
                                    y: -14
                                )
                        }
                    }
                }
                .frame(height: 8)
```

Note: The `.frame(height: 8)` stays on the GeometryReader. The animal overflows via offset but doesn't affect layout. We need to add top padding to the VStack to accommodate the animal. Update the VStack (line 17):

```swift
        VStack(alignment: .leading, spacing: 4) {
```

And wrap the whole body in a container with top padding when animal is present:

```swift
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(width: 24, alignment: .leading)

                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.primary.opacity(0.1))

                        RoundedRectangle(cornerRadius: 4)
                            .fill(DashboardViewModel.usageColor(for: utilization))
                            .frame(width: geo.size.width * min(utilization / 100, 1.0))

                        if let animal {
                            Text(animal)
                                .font(.system(size: 12))
                                .offset(
                                    x: geo.size.width * min(utilization / 100, 1.0) - 8,
                                    y: -14
                                )
                        }
                    }
                }
                .frame(height: 8)

                Text("\(Int(utilization))%")
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 36, alignment: .trailing)
            }
            .padding(.top, animal != nil ? 14 : 0)

            if let resetsAt {
                Text("resets in \(formatTimeRemaining(resetsAt))")
                    .font(.caption2)
                    .foregroundStyle(resetUrgencyColor(resetsAt))
                    .padding(.leading, 28)
            }
        }
    }
```

- [ ] **Step 3: Build to verify compilation**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`
Expected: BUILD SUCCEEDED

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/Views/UsageBar.swift
git commit -m "feat: add animal emoji overlay on UsageBar progress bar"
```

---

### Task 7: AccountCard — Pass Animal + Tap for Drill-down

**Files:**
- Modify: `ClaudeDashboard/Views/AccountCard.swift`

- [ ] **Step 1: Add onTap callback and pass animal to UsageBar**

Update struct properties (lines 3-6):

```swift
struct AccountCard: View {
    let state: AccountUsageState
    let onResync: () -> Void
    let onTogglePin: () -> Void
    var onTap: (() -> Void)? = nil
```

Update `usageContent` (lines 81-89) to pass animal:

```swift
    private func usageContent(_ usage: UsageData) -> some View {
        VStack(spacing: 8) {
            UsageBar(label: "5h", utilization: usage.fiveHour.utilization, resetsAt: usage.fiveHour.resetsAt, totalSeconds: 18000, animal: state.burnRates?.fiveHour?.animal)
            UsageBar(label: "7d", utilization: usage.sevenDay.utilization, resetsAt: usage.sevenDay.resetsAt, totalSeconds: 604800, animal: state.burnRates?.sevenDay?.animal)
            if let sonnet = usage.sevenDaySonnet {
                UsageBar(label: "S", utilization: sonnet.utilization, resetsAt: sonnet.resetsAt, totalSeconds: 604800, animal: state.burnRates?.sonnet?.animal)
            }
        }
    }
```

Add tap gesture on the GroupBox. After `.contextMenu { ... }` (line 78), add:

```swift
        .contentShape(Rectangle())
        .onTapGesture {
            onTap?()
        }
```

- [ ] **Step 2: Build to verify compilation**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`
Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/AccountCard.swift
git commit -m "feat: pass burn rate animals to UsageBar, add tap gesture on AccountCard"
```

---

### Task 8: AccountDetailViewModel

**Files:**
- Create: `ClaudeDashboard/ViewModels/AccountDetailViewModel.swift`

- [ ] **Step 1: Create the ViewModel**

```swift
// ClaudeDashboard/ViewModels/AccountDetailViewModel.swift
import Foundation
import SwiftUI

@MainActor
final class AccountDetailViewModel: ObservableObject {
    let accountId: UUID
    let accountName: String
    let accountPlan: AccountPlan
    private let logStore: UsageLogStore

    @Published var selectedWindow: UsageWindow = .fiveHour
    @Published var logs: [UsageLogEntry] = []
    @Published var resetCycles: [ResetCycle] = []
    @Published var selectedCycle: ResetCycle?

    init(accountId: UUID, accountName: String, accountPlan: AccountPlan, logStore: UsageLogStore) {
        self.accountId = accountId
        self.accountName = accountName
        self.accountPlan = accountPlan
        self.logStore = logStore
    }

    func loadData() async {
        let cycles = await logStore.resetCycles(accountId: accountId, window: selectedWindow)
        resetCycles = cycles

        if let cycle = selectedCycle {
            let cycleLogs = await logStore.logs(
                accountId: accountId, window: selectedWindow,
                from: cycle.firstRecordedAt.addingTimeInterval(-1),
                to: cycle.resetsAt
            )
            logs = cycleLogs
        } else {
            let allLogs = await logStore.logs(
                accountId: accountId, window: selectedWindow,
                from: nil, to: nil
            )
            logs = allLogs
        }
    }

    func selectWindow(_ window: UsageWindow) {
        selectedWindow = window
        selectedCycle = nil
        Task { await loadData() }
    }

    func selectCycle(_ cycle: ResetCycle?) {
        selectedCycle = cycle
        Task { await loadData() }
    }
}
```

- [ ] **Step 2: Build to verify compilation**

Run: `xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`
Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/ViewModels/AccountDetailViewModel.swift
git commit -m "feat: add AccountDetailViewModel for drill-down chart data"
```

---

### Task 9: AccountDetailView — Drill-down Chart

**Files:**
- Create: `ClaudeDashboard/Views/AccountDetailView.swift`
- Modify: `project.yml` (add Charts framework)

- [ ] **Step 1: Add Charts framework to project.yml**

In `project.yml`, add under `ClaudeDashboard` target's `dependencies` (after line 28):

```yaml
                - sdk: IOKit.framework
                - framework: Charts.framework
                  embed: false
```

Actually, Swift Charts is imported as a module, not a linked framework. Just importing `Charts` in Swift is enough if the deployment target is macOS 13+. No `project.yml` change needed — just `import Charts` in the Swift file.

- [ ] **Step 2: Create AccountDetailView**

```swift
// ClaudeDashboard/Views/AccountDetailView.swift
import SwiftUI
import Charts

struct AccountDetailView: View {
    @StateObject var viewModel: AccountDetailViewModel
    let onBack: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Button(action: onBack) {
                    Label("Back", systemImage: "chevron.left")
                }
                .buttonStyle(.borderless)

                Text(viewModel.accountName)
                    .font(.title2.bold())

                Spacer()

                Text(viewModel.accountPlan.rawValue)
                    .font(.caption2.bold())
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(viewModel.accountPlan.badgeColor.opacity(0.15))
                    .clipShape(Capsule())
            }
            .padding()

            Divider()

            // Window picker
            HStack {
                Picker("Window", selection: Binding(
                    get: { viewModel.selectedWindow },
                    set: { viewModel.selectWindow($0) }
                )) {
                    Text("5h").tag(UsageWindow.fiveHour)
                    Text("7d").tag(UsageWindow.sevenDay)
                    Text("S").tag(UsageWindow.sonnet)
                }
                .pickerStyle(.segmented)
                .frame(width: 180)

                Spacer()

                if viewModel.selectedCycle != nil {
                    Button("Show All") {
                        viewModel.selectCycle(nil)
                    }
                    .buttonStyle(.borderless)
                    .font(.caption)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 8)

            // Chart
            if viewModel.logs.isEmpty {
                VStack(spacing: 8) {
                    Spacer()
                    Image(systemName: "chart.line.downtrend.xyaxis")
                        .font(.system(size: 36))
                        .foregroundStyle(.secondary)
                    Text("No data yet")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Text("Data will appear after the next refresh.")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    Spacer()
                }
                .frame(maxWidth: .infinity)
            } else {
                usageChart
                    .padding()
            }

            // Reset cycles list
            if !viewModel.resetCycles.isEmpty {
                Divider()
                resetCyclesList
            }
        }
        .task { await viewModel.loadData() }
    }

    private var usageChart: some View {
        Chart {
            ForEach(viewModel.logs) { log in
                LineMark(
                    x: .value("Time", log.recordedAt),
                    y: .value("Usage", log.utilization)
                )
                .foregroundStyle(Color.blue)
                .interpolationMethod(.monotone)

                if log.isLimited {
                    PointMark(
                        x: .value("Time", log.recordedAt),
                        y: .value("Usage", log.utilization)
                    )
                    .foregroundStyle(Color.red)
                    .annotation(position: .top) {
                        Text("⚠")
                            .font(.caption2)
                    }
                }
            }

            RuleMark(y: .value("Limit", 100))
                .foregroundStyle(.red.opacity(0.3))
                .lineStyle(StrokeStyle(dash: [5, 5]))
        }
        .chartYScale(domain: 0...105)
        .chartYAxis {
            AxisMarks(values: [0, 25, 50, 75, 100]) { value in
                AxisGridLine()
                AxisValueLabel {
                    if let v = value.as(Int.self) {
                        Text("\(v)%")
                    }
                }
            }
        }
        .frame(height: 250)
    }

    private var resetCyclesList: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Reset Cycles")
                .font(.caption.bold())
                .foregroundStyle(.secondary)
                .padding(.horizontal)
                .padding(.top, 8)

            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(viewModel.resetCycles) { cycle in
                        Button {
                            viewModel.selectCycle(cycle)
                        } label: {
                            HStack {
                                Circle()
                                    .fill(DashboardViewModel.usageColor(for: cycle.peakUtilization))
                                    .frame(width: 8, height: 8)

                                Text(formatCycleRange(cycle))
                                    .font(.caption)

                                Spacer()

                                Text("peak: \(Int(cycle.peakUtilization))%")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)

                                Text("\(cycle.dataPointCount) pts")
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                            .padding(.horizontal)
                            .padding(.vertical, 6)
                            .background(
                                viewModel.selectedCycle?.resetsAt == cycle.resetsAt
                                    ? Color.accentColor.opacity(0.1)
                                    : Color.clear
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .frame(maxHeight: 150)
        }
    }

    private func formatCycleRange(_ cycle: ResetCycle) -> String {
        let df = DateFormatter()
        df.dateFormat = "d MMM HH:mm"
        return "\(df.string(from: cycle.firstRecordedAt)) – \(df.string(from: cycle.resetsAt))"
    }
}
```

- [ ] **Step 3: Build to verify compilation**

Run: `xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`
Expected: BUILD SUCCEEDED

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/Views/AccountDetailView.swift
git commit -m "feat: add AccountDetailView with Swift Charts drill-down"
```

---

### Task 10: OverviewChartView — Aggregated Multi-Account

**Files:**
- Create: `ClaudeDashboard/Views/OverviewChartView.swift`

- [ ] **Step 1: Create OverviewChartView**

```swift
// ClaudeDashboard/Views/OverviewChartView.swift
import SwiftUI
import Charts

struct OverviewChartView: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onBack: () -> Void

    @State private var selectedWindow: UsageWindow = .fiveHour
    @State private var timeRange: TimeRange = .day
    @State private var selectedAccounts: Set<UUID> = []
    @State private var logs: [UsageLogEntry] = []

    enum TimeRange: String, CaseIterable {
        case day = "24h"
        case threeDay = "3d"
        case week = "7d"
        case month = "30d"

        var seconds: TimeInterval {
            switch self {
            case .day: return 86400
            case .threeDay: return 3 * 86400
            case .week: return 7 * 86400
            case .month: return 30 * 86400
            }
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Button(action: onBack) {
                    Label("Back", systemImage: "chevron.left")
                }
                .buttonStyle(.borderless)

                Text("Overview")
                    .font(.title2.bold())

                Spacer()
            }
            .padding()

            Divider()

            // Controls
            HStack {
                Picker("Window", selection: $selectedWindow) {
                    Text("5h").tag(UsageWindow.fiveHour)
                    Text("7d").tag(UsageWindow.sevenDay)
                    Text("S").tag(UsageWindow.sonnet)
                }
                .pickerStyle(.segmented)
                .frame(width: 180)

                Spacer()

                Picker("Time", selection: $timeRange) {
                    ForEach(TimeRange.allCases, id: \.self) { range in
                        Text(range.rawValue).tag(range)
                    }
                }
                .frame(width: 100)
            }
            .padding(.horizontal)
            .padding(.vertical, 8)

            // Chart
            if logs.isEmpty {
                VStack(spacing: 8) {
                    Spacer()
                    Image(systemName: "chart.line.downtrend.xyaxis")
                        .font(.system(size: 36))
                        .foregroundStyle(.secondary)
                    Text("No data yet")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                .frame(maxWidth: .infinity)
            } else {
                overviewChart
                    .padding()
            }

            Divider()

            // Legend with toggles
            legendView
        }
        .task { await loadLogs() }
        .onChange(of: selectedWindow) { _ in Task { await loadLogs() } }
        .onChange(of: timeRange) { _ in Task { await loadLogs() } }
    }

    private var overviewChart: some View {
        Chart {
            // Per-account lines
            ForEach(viewModel.accountStates.filter { selectedAccounts.contains($0.id) }) { state in
                let accountLogs = logs.filter { $0.accountId == state.id }
                ForEach(accountLogs) { log in
                    LineMark(
                        x: .value("Time", log.recordedAt),
                        y: .value("Usage", log.utilization),
                        series: .value("Account", state.account.name)
                    )
                    .foregroundStyle(by: .value("Account", state.account.name))
                    .lineStyle(StrokeStyle(lineWidth: 1.5))
                    .interpolationMethod(.monotone)
                }
            }

            // Total line
            ForEach(computeTotalLine(), id: \.time) { point in
                LineMark(
                    x: .value("Time", point.time),
                    y: .value("Usage", point.value),
                    series: .value("Account", "Total")
                )
                .foregroundStyle(.primary)
                .lineStyle(StrokeStyle(lineWidth: 3))
                .interpolationMethod(.monotone)
            }

            // Limit markers
            ForEach(logs.filter { $0.isLimited && selectedAccounts.contains($0.accountId) }) { log in
                PointMark(
                    x: .value("Time", log.recordedAt),
                    y: .value("Usage", log.utilization)
                )
                .foregroundStyle(.red)
                .annotation(position: .top) {
                    Text("⚠").font(.caption2)
                }
            }
        }
        .chartYScale(domain: 0...105)
        .chartYAxis {
            AxisMarks(values: [0, 25, 50, 75, 100]) { value in
                AxisGridLine()
                AxisValueLabel {
                    if let v = value.as(Int.self) { Text("\(v)%") }
                }
            }
        }
        .frame(height: 300)
    }

    private var legendView: some View {
        ScrollView {
            VStack(spacing: 4) {
                // Total row
                HStack {
                    Image(systemName: "checkmark.square.fill")
                        .foregroundStyle(.primary)
                    Text("Total")
                        .font(.caption.bold())
                    Spacer()
                }
                .padding(.horizontal)
                .padding(.vertical, 4)

                ForEach(viewModel.accountStates) { state in
                    Button {
                        if selectedAccounts.contains(state.id) {
                            selectedAccounts.remove(state.id)
                        } else {
                            selectedAccounts.insert(state.id)
                        }
                        Task { await loadLogs() }
                    } label: {
                        HStack {
                            Image(systemName: selectedAccounts.contains(state.id) ? "checkmark.square.fill" : "square")
                                .foregroundStyle(selectedAccounts.contains(state.id) ? .accentColor : .secondary)
                            Text(state.account.name)
                                .font(.caption)
                            if let email = state.account.email, email != state.account.name {
                                Text(email)
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                            Spacer()
                            if let animal = state.burnRates?.fiveHour?.animal {
                                Text(animal)
                            } else {
                                Text("—")
                                    .foregroundStyle(.tertiary)
                            }
                        }
                        .padding(.horizontal)
                        .padding(.vertical, 4)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .frame(maxHeight: 120)
    }

    struct TotalPoint: Identifiable {
        let time: Date
        let value: Double
        var id: Date { time }
    }

    private func computeTotalLine() -> [TotalPoint] {
        let selected = viewModel.accountStates.filter { selectedAccounts.contains($0.id) }
        guard !selected.isEmpty else { return [] }

        // Gather all unique timestamps from selected account logs
        let selectedLogs = logs.filter { selectedAccounts.contains($0.accountId) }
        let allTimes = Set(selectedLogs.map { $0.recordedAt }).sorted()

        // Plan weights
        let weights: [UUID: Double] = Dictionary(uniqueKeysWithValues: selected.map { state in
            let w: Double
            switch state.account.plan {
            case .pro: w = 1
            case .max5x: w = 5
            case .max20x: w = 20
            case .max200: w = 10
            }
            return (state.id, w)
        })

        return allTimes.map { time in
            var weightedSum = 0.0
            var totalWeight = 0.0

            for state in selected {
                let accountLogs = selectedLogs.filter { $0.accountId == state.id }
                if let utilization = interpolate(at: time, in: accountLogs) {
                    let w = weights[state.id] ?? 1
                    weightedSum += utilization * w
                    totalWeight += w
                }
            }

            let avg = totalWeight > 0 ? weightedSum / totalWeight : 0
            return TotalPoint(time: time, value: avg)
        }
    }

    private func interpolate(at time: Date, in logs: [UsageLogEntry]) -> Double? {
        guard !logs.isEmpty else { return nil }

        // Exact match
        if let exact = logs.first(where: { $0.recordedAt == time }) {
            return exact.utilization
        }

        // Find surrounding points
        let before = logs.last(where: { $0.recordedAt <= time })
        let after = logs.first(where: { $0.recordedAt >= time })

        if let b = before, let a = after, b.recordedAt != a.recordedAt {
            let fraction = time.timeIntervalSince(b.recordedAt) / a.recordedAt.timeIntervalSince(b.recordedAt)
            return b.utilization + (a.utilization - b.utilization) * fraction
        }

        return before?.utilization ?? after?.utilization
    }

    private func loadLogs() async {
        if selectedAccounts.isEmpty {
            selectedAccounts = Set(viewModel.accountStates.map(\.id))
        }

        let from = Date().addingTimeInterval(-timeRange.seconds)
        let store = viewModel.logStore
        logs = await store.allLogs(window: selectedWindow, from: from, to: nil)
    }
}
```

- [ ] **Step 2: Build to verify compilation**

Run: `xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`
Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/OverviewChartView.swift
git commit -m "feat: add OverviewChartView with aggregated multi-account chart"
```

---

### Task 11: Navigation — DashboardWindow + MenuBarPopover

**Files:**
- Modify: `ClaudeDashboard/Views/DashboardWindow.swift`
- Modify: `ClaudeDashboard/Views/MenuBarPopover.swift`
- Modify: `ClaudeDashboard/ViewModels/DashboardViewModel.swift`

- [ ] **Step 1: Add navigation state to DashboardViewModel**

Add navigation enum and state property to `DashboardViewModel.swift`, after the `@Published var autoRefreshMinutes` (around line 23):

```swift
    enum NavigationDestination: Equatable {
        case dashboard
        case accountDetail(UUID)
        case overview
    }

    @Published var navigation: NavigationDestination = .dashboard
```

Note: `logStore` is already stored directly on DashboardViewModel from Task 5.

- [ ] **Step 2: Update DashboardWindow with navigation and Overview button**

Replace the entire `DashboardWindow` body:

```swift
struct DashboardWindow: View {
    @ObservedObject var viewModel: DashboardViewModel
    @State private var showingSettings = false

    var body: some View {
        Group {
            switch viewModel.navigation {
            case .dashboard:
                dashboardContent
            case .accountDetail(let accountId):
                if let state = viewModel.accountStates.first(where: { $0.id == accountId }) {
                    AccountDetailView(
                        viewModel: AccountDetailViewModel(
                            accountId: accountId,
                            accountName: state.account.name,
                            accountPlan: state.account.plan,
                            logStore: viewModel.logStore
                        ),
                        onBack: { viewModel.navigation = .dashboard }
                    )
                }
            case .overview:
                OverviewChartView(
                    viewModel: viewModel,
                    onBack: { viewModel.navigation = .dashboard }
                )
            }
        }
        .frame(minWidth: 400, minHeight: 300)
        .sheet(isPresented: $showingSettings) {
            SettingsView(viewModel: viewModel)
        }
    }

    private var dashboardContent: some View {
        VStack(spacing: 0) {
            // Toolbar
            HStack {
                Text("Claude Dashboard")
                    .font(.title2.bold())

                Spacer()

                Button(action: { viewModel.navigation = .overview }) {
                    Label("Overview", systemImage: "chart.xyaxis.line")
                }

                Button(action: {
                    Task { await viewModel.refreshAll() }
                }) {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(viewModel.isRefreshing)

                Button(action: { showingSettings = true }) {
                    Label("Settings", systemImage: "gearshape")
                }
            }
            .padding()

            Divider()

            // Cards grid
            if viewModel.accountStates.isEmpty {
                emptyStateView
            } else {
                ScrollView {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 280, maximum: 400), spacing: 12)],
                        spacing: 12
                    ) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state, onResync: {
                                Task { await viewModel.resyncAccount(state.id) }
                            }, onTogglePin: {
                                viewModel.togglePin(for: state.id)
                            }, onTap: {
                                viewModel.navigation = .accountDetail(state.id)
                            })
                        }
                    }
                    .padding()
                }
            }
        }
    }

    private var emptyStateView: some View {
        VStack(spacing: 12) {
            Spacer()
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No Accounts")
                .font(.title3.bold())
            Text("Open Settings to sync accounts from Chrome.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Spacer()
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
```

- [ ] **Step 3: Update MenuBarPopover — add tap on AccountCard**

In `MenuBarPopover.swift`, update the AccountCard in the ForEach (around line 53):

```swift
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state, onResync: {
                                Task { await viewModel.resyncAccount(state.id) }
                            }, onTogglePin: {
                                viewModel.togglePin(for: state.id)
                            })
                        }
```

No tap handler in popover — the popover is too small for drill-down. Drill-down is only in the full DashboardWindow.

- [ ] **Step 4: Build to verify compilation**

Run: `xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`
Expected: BUILD SUCCEEDED

- [ ] **Step 5: Run all tests**

Run: `xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests 2>&1 | tail -30`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add ClaudeDashboard/Views/DashboardWindow.swift ClaudeDashboard/Views/MenuBarPopover.swift ClaudeDashboard/ViewModels/DashboardViewModel.swift
git commit -m "feat: add navigation to AccountDetailView and OverviewChartView"
```

---

### Task 12: project.yml — Ensure Charts Framework Available

**Files:**
- Modify: `project.yml`

- [ ] **Step 1: Verify Charts import works**

Swift Charts is part of the SDK on macOS 13+. No additional framework linking is needed in `project.yml` — `import Charts` in Swift files is sufficient. Verify by building:

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | grep -i "charts\|error" | head -10`
Expected: No errors related to Charts

If Charts import fails, add to `project.yml` under `ClaudeDashboard` target settings:

```yaml
                - sdk: Charts.framework
```

- [ ] **Step 2: Commit if project.yml changed**

```bash
# Only if changes were needed:
git add project.yml
git commit -m "build: add Charts framework dependency"
```

---

### Task 13: Final Integration Test — Full Build + Run All Tests

**Files:** None (verification only)

- [ ] **Step 1: Regenerate Xcode project**

Run: `xcodegen generate`
Expected: Generated project at ClaudeDashboard.xcodeproj

- [ ] **Step 2: Full build**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`
Expected: BUILD SUCCEEDED

- [ ] **Step 3: Run all tests**

Run: `xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests 2>&1 | tail -30`
Expected: All tests PASS

- [ ] **Step 4: Final commit if any fixes were needed**

```bash
git add -A
git commit -m "fix: resolve integration issues from burn rate + logging feature"
```
