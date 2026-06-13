// ClaudeDashboard/Services/CommandLogStore.swift
import Foundation
import SQLite3

private let SQLITE_TRANSIENT = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

/// Persists command executions to its own SQLite file, capped at `maxEntries`
/// most-recent rows. Decoupled from UsageLogStore (separate db, no shared schema).
actor CommandLogStore {
    private var db: OpaquePointer?
    private let maxEntries: Int

    init(dbPath: String? = nil, maxEntries: Int = 500) {
        self.maxEntries = maxEntries
        let path = dbPath ?? CommandLogStore.defaultDBPath()
        openDatabase(at: path)
        createTables()
    }

    deinit {
        if let db { sqlite3_close(db) }
    }

    // MARK: - Record

    @discardableResult
    func record(accountId: UUID?, command: String, trigger: CommandTrigger,
                startedAt: Date, finishedAt: Date?, exitCode: Int32?) -> Int64 {
        let sql = """
        INSERT INTO command_logs (account_id, cmd, trig, started, finished, exit_code)
        VALUES (?, ?, ?, ?, ?, ?)
        """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return -1 }
        defer { sqlite3_finalize(stmt) }

        if let accountId {
            accountId.uuidString.withCString { sqlite3_bind_text(stmt, 1, $0, -1, SQLITE_TRANSIENT) }
        } else {
            sqlite3_bind_null(stmt, 1)
        }
        command.withCString { sqlite3_bind_text(stmt, 2, $0, -1, SQLITE_TRANSIENT) }
        sqlite3_bind_int(stmt, 3, Int32(trigger.rawValue))
        sqlite3_bind_int64(stmt, 4, Int64(startedAt.timeIntervalSince1970))
        if let finishedAt {
            sqlite3_bind_int64(stmt, 5, Int64(finishedAt.timeIntervalSince1970))
        } else {
            sqlite3_bind_null(stmt, 5)
        }
        if let exitCode {
            sqlite3_bind_int(stmt, 6, exitCode)
        } else {
            sqlite3_bind_null(stmt, 6)
        }

        guard sqlite3_step(stmt) == SQLITE_DONE else { return -1 }
        let id = sqlite3_last_insert_rowid(db)
        prune()
        return id
    }

    // MARK: - Query

    func recent(limit: Int) -> [CommandLogEntry] {
        let sql = """
        SELECT id, account_id, cmd, trig, started, finished, exit_code
        FROM command_logs ORDER BY id DESC LIMIT ?
        """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_int(stmt, 1, Int32(limit))

        var results: [CommandLogEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            let accountId: UUID? = {
                guard let c = sqlite3_column_text(stmt, 1) else { return nil }
                return UUID(uuidString: String(cString: c))
            }()
            let command = sqlite3_column_text(stmt, 2).map { String(cString: $0) } ?? ""
            let trigger = CommandTrigger(rawValue: Int(sqlite3_column_int(stmt, 3))) ?? .manual
            let started = Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 4)))
            let finished: Date? = sqlite3_column_type(stmt, 5) == SQLITE_NULL
                ? nil : Date(timeIntervalSince1970: TimeInterval(sqlite3_column_int64(stmt, 5)))
            let exitCode: Int32? = sqlite3_column_type(stmt, 6) == SQLITE_NULL
                ? nil : sqlite3_column_int(stmt, 6)
            results.append(CommandLogEntry(
                id: sqlite3_column_int64(stmt, 0),
                accountId: accountId,
                command: command,
                trigger: trigger,
                startedAt: started,
                finishedAt: finished,
                exitCode: exitCode
            ))
        }
        return results
    }

    func clear() {
        sqlite3_exec(db, "DELETE FROM command_logs", nil, nil, nil)
    }

    // MARK: - Retention

    private func prune() {
        let sql = "DELETE FROM command_logs WHERE id NOT IN (SELECT id FROM command_logs ORDER BY id DESC LIMIT ?)"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_int(stmt, 1, Int32(maxEntries))
        sqlite3_step(stmt)
    }

    // MARK: - Database Setup

    private func openDatabase(at path: String) {
        let dir = (path as NSString).deletingLastPathComponent
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        guard sqlite3_open(path, &db) == SQLITE_OK else {
            print("[CommandLogStore] Failed to open database at \(path)")
            return
        }
    }

    private func createTables() {
        let sql = """
        CREATE TABLE IF NOT EXISTS command_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id TEXT,
            cmd TEXT NOT NULL,
            trig INTEGER NOT NULL,
            started INTEGER NOT NULL,
            finished INTEGER,
            exit_code INTEGER
        )
        """
        sqlite3_exec(db, sql, nil, nil, nil)
    }

    static func defaultDBPath() -> String {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let dir = appSupport.appendingPathComponent("ClaudeDashboard")
        return dir.appendingPathComponent("command_logs.db").path
    }
}
