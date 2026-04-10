// ClaudeDashboard/Services/UsageLogStore.swift
import Foundation
import SQLite3

private let SQLITE_TRANSIENT = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

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
        guard let aid = resolveAccountId(accountId) else { return }
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

    private func resolveAccountId(_ uuid: UUID) -> Int32? {
        if let existing = lookupAccountId(uuid) { return existing }
        let sql = "INSERT INTO accounts_map (account_id) VALUES (?)"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_finalize(stmt) }
        let str = uuid.uuidString
        str.withCString { cStr in
            sqlite3_bind_text(stmt, 1, cStr, -1, SQLITE_TRANSIENT)
        }
        guard sqlite3_step(stmt) == SQLITE_DONE else { return nil }
        return Int32(sqlite3_last_insert_rowid(db))
    }

    private func lookupAccountId(_ uuid: UUID) -> Int32? {
        let sql = "SELECT aid FROM accounts_map WHERE account_id = ?"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_finalize(stmt) }
        let str = uuid.uuidString
        str.withCString { cStr in
            sqlite3_bind_text(stmt, 1, cStr, -1, SQLITE_TRANSIENT)
        }
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
