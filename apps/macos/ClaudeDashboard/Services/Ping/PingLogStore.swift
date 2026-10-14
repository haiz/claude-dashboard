import Foundation
import SQLite3

private let SQLITE_TRANSIENT = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

/// Persists finished pings to `ping_logs.db`, keeping the newest `maxEntries` rows.
/// One row per ping, written when it finishes.
///
///     let store = PingLogStore()
///     await store.insert(accountId: id, trigger: .manual, startedAt: start,
///                        finishedAt: Date(), result: result)
///     await store.latest(for: id)?.outcome   // .ok
actor PingLogStore {
    private var db: OpaquePointer?
    private let maxEntries: Int
    private static let columns = "id, account_id, trig, started, finished, outcome, detail"

    init(dbPath: String? = nil, maxEntries: Int = 500) {
        self.maxEntries = maxEntries
        openDatabase(at: dbPath ?? PingLogStore.defaultDBPath())
        createTable()
    }

    deinit {
        if let db { sqlite3_close(db) }
    }

    // MARK: - Write

    @discardableResult
    func insert(accountId: UUID, trigger: PingTrigger, startedAt: Date, finishedAt: Date,
                result: PingResult) -> Int64 {
        let sql = """
        INSERT INTO ping_logs (account_id, trig, started, finished, outcome, detail)
        VALUES (?, ?, ?, ?, ?, ?)
        """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return -1 }
        defer { sqlite3_finalize(stmt) }

        accountId.uuidString.withCString { sqlite3_bind_text(stmt, 1, $0, -1, SQLITE_TRANSIENT) }
        sqlite3_bind_int(stmt, 2, Int32(trigger.rawValue))
        sqlite3_bind_double(stmt, 3, startedAt.timeIntervalSince1970)
        sqlite3_bind_double(stmt, 4, finishedAt.timeIntervalSince1970)
        sqlite3_bind_int(stmt, 5, Int32(result.outcome.rawValue))
        if let detail = result.detail {
            detail.withCString { sqlite3_bind_text(stmt, 6, $0, -1, SQLITE_TRANSIENT) }
        } else {
            sqlite3_bind_null(stmt, 6)
        }

        guard sqlite3_step(stmt) == SQLITE_DONE else { return -1 }
        let id = sqlite3_last_insert_rowid(db)
        prune()
        return id
    }

    func clear() {
        sqlite3_exec(db, "DELETE FROM ping_logs", nil, nil, nil)
    }

    // MARK: - Read

    func fetch(limit: Int) -> [PingLogEntry] {
        query("SELECT \(Self.columns) FROM ping_logs ORDER BY id DESC LIMIT ?") { stmt in
            _ = sqlite3_bind_int(stmt, 1, Int32(limit))
        }
    }

    func latest(for accountId: UUID) -> PingLogEntry? {
        query("SELECT \(Self.columns) FROM ping_logs WHERE account_id = ? ORDER BY id DESC LIMIT 1") { stmt in
            _ = accountId.uuidString.withCString { sqlite3_bind_text(stmt, 1, $0, -1, SQLITE_TRANSIENT) }
        }.first
    }

    private func query(_ sql: String, bind: (OpaquePointer?) -> Void) -> [PingLogEntry] {
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }
        bind(stmt)

        var rows: [PingLogEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            guard let idText = sqlite3_column_text(stmt, 1),
                  let accountId = UUID(uuidString: String(cString: idText)),
                  let trigger = PingTrigger(rawValue: Int(sqlite3_column_int(stmt, 2))),
                  let outcome = PingOutcome(rawValue: Int(sqlite3_column_int(stmt, 5))) else { continue }
            rows.append(PingLogEntry(
                id: sqlite3_column_int64(stmt, 0),
                accountId: accountId,
                trigger: trigger,
                startedAt: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 3)),
                finishedAt: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 4)),
                outcome: outcome,
                detail: sqlite3_column_text(stmt, 6).map { String(cString: $0) }
            ))
        }
        return rows
    }

    // MARK: - Retention

    private func prune() {
        let sql = "DELETE FROM ping_logs WHERE id NOT IN (SELECT id FROM ping_logs ORDER BY id DESC LIMIT ?)"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_int(stmt, 1, Int32(maxEntries))
        sqlite3_step(stmt)
    }

    // MARK: - Setup

    private func openDatabase(at path: String) {
        let dir = (path as NSString).deletingLastPathComponent
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        guard sqlite3_open(path, &db) == SQLITE_OK else {
            print("[PingLogStore] Failed to open database at \(path)")
            return
        }
    }

    private func createTable() {
        let sql = """
        CREATE TABLE IF NOT EXISTS ping_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id TEXT NOT NULL,
            trig INTEGER NOT NULL,
            started REAL NOT NULL,
            finished REAL NOT NULL,
            outcome INTEGER NOT NULL,
            detail TEXT
        )
        """
        sqlite3_exec(db, sql, nil, nil, nil)
    }

    /// `~/Library/Application Support/ClaudeDashboard`, shared with the other logs.
    static func defaultDirectory() -> URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("ClaudeDashboard")
    }

    static func defaultDBPath() -> String {
        defaultDirectory().appendingPathComponent("ping_logs.db").path
    }
}
