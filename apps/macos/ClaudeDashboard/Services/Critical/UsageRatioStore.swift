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
