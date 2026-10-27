import Foundation
import SQLite3

private let SQLITE_TRANSIENT = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

/// Persists auto and manual switch events to `auto_switch_logs.db`, keeping the newest `maxEntries` rows.
///
///     let store = AutoSwitchLogStore()
///     await store.insert(date: Date(), trigger: .auto, outcome: .switched, fromName: "a@x.com",
///                        toName: "b@x.com", reason: "5h 96% >= 95%, tier 1", detail: nil)   // target defaults to .claudeCode
///     await store.fetch(limit: 50).first?.outcome   // .switched
actor AutoSwitchLogStore {
    private var db: OpaquePointer?
    private let maxEntries: Int

    init(dbPath: String? = nil, maxEntries: Int = 500) {
        self.maxEntries = maxEntries
        openDatabase(at: dbPath ?? AutoSwitchLogStore.defaultDBPath())
        createTable()
    }

    deinit {
        if let db { sqlite3_close(db) }
    }

    // MARK: - Write

    @discardableResult
    func insert(date: Date, trigger: AutoSwitchTrigger, outcome: AutoSwitchOutcome, fromName: String?, toName: String?,
                reason: String, detail: String?, target: SwitchTarget = .claudeCode) -> Int64 {
        let sql = """
        INSERT INTO auto_switch_logs (date, outcome, from_name, to_name, reason, detail, trig, target)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return -1 }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_double(stmt, 1, date.timeIntervalSince1970)
        sqlite3_bind_int(stmt, 2, Int32(outcome.rawValue))
        bind(fromName, at: 3, in: stmt)
        bind(toName, at: 4, in: stmt)
        bind(reason, at: 5, in: stmt)
        bind(detail, at: 6, in: stmt)
        sqlite3_bind_int(stmt, 7, Int32(trigger.rawValue))
        sqlite3_bind_int(stmt, 8, Int32(target.rawValue))

        guard sqlite3_step(stmt) == SQLITE_DONE else { return -1 }
        let id = sqlite3_last_insert_rowid(db)
        prune()
        return id
    }

    func clear() {
        sqlite3_exec(db, "DELETE FROM auto_switch_logs", nil, nil, nil)
    }

    private func bind(_ text: String?, at index: Int32, in stmt: OpaquePointer?) {
        if let text {
            text.withCString { _ = sqlite3_bind_text(stmt, index, $0, -1, SQLITE_TRANSIENT) }
        } else {
            sqlite3_bind_null(stmt, index)
        }
    }

    // MARK: - Read

    func fetch(limit: Int) -> [AutoSwitchLogEntry] {
        let sql = """
        SELECT id, date, outcome, from_name, to_name, reason, detail, trig, target
        FROM auto_switch_logs ORDER BY id DESC LIMIT ?
        """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_int(stmt, 1, Int32(limit))

        var rows: [AutoSwitchLogEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            guard let outcome = AutoSwitchOutcome(rawValue: Int(sqlite3_column_int(stmt, 2))),
                  let trigger = AutoSwitchTrigger(rawValue: Int(sqlite3_column_int(stmt, 7))),
                  let target = SwitchTarget(rawValue: Int(sqlite3_column_int(stmt, 8))) else { continue }
            rows.append(AutoSwitchLogEntry(
                id: sqlite3_column_int64(stmt, 0),
                date: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 1)),
                trigger: trigger,
                outcome: outcome,
                fromName: sqlite3_column_text(stmt, 3).map { String(cString: $0) },
                toName: sqlite3_column_text(stmt, 4).map { String(cString: $0) },
                reason: sqlite3_column_text(stmt, 5).map { String(cString: $0) } ?? "",
                detail: sqlite3_column_text(stmt, 6).map { String(cString: $0) },
                target: target
            ))
        }
        return rows
    }

    // MARK: - Retention

    private func prune() {
        let sql = """
        DELETE FROM auto_switch_logs WHERE id NOT IN
            (SELECT id FROM auto_switch_logs ORDER BY id DESC LIMIT ?)
        """
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
            print("[AutoSwitchLogStore] Failed to open database at \(path)")
            return
        }
    }

    private func createTable() {
        let sql = """
        CREATE TABLE IF NOT EXISTS auto_switch_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date REAL NOT NULL,
            outcome INTEGER NOT NULL,
            from_name TEXT,
            to_name TEXT,
            reason TEXT NOT NULL,
            detail TEXT,
            trig INTEGER NOT NULL DEFAULT 0,
            target INTEGER NOT NULL DEFAULT 0
        )
        """
        sqlite3_exec(db, sql, nil, nil, nil)
        // A table from before manual switches were logged: add the column (its rows are all
        // auto). Fails harmlessly with "duplicate column" once it exists.
        sqlite3_exec(db, "ALTER TABLE auto_switch_logs ADD COLUMN trig INTEGER NOT NULL DEFAULT 0", nil, nil, nil)
        // Same for the column that tells Claude Code switches from Claude Desktop ones (old rows
        // are all Claude Code).
        sqlite3_exec(db, "ALTER TABLE auto_switch_logs ADD COLUMN target INTEGER NOT NULL DEFAULT 0", nil, nil, nil)
    }

    static func defaultDBPath() -> String {
        PingLogStore.defaultDirectory().appendingPathComponent("auto_switch_logs.db").path
    }
}
