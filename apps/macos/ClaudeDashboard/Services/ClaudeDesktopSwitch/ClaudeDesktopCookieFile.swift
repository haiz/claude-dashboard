import CryptoKit
import Foundation
import SQLite3

/// Claude Desktop's claude.ai web session. Desktop signs in as the account of this
/// `sessionKey` cookie (its webview and bootstrap fetch report it), not the account named in
/// `config.json`, so a switch must replace it too.
protocol ClaudeDesktopSessionWriting {
    /// The `sessionKey` Desktop holds; nil when the database or the row is absent or unreadable.
    func readSessionKey() -> String?
    /// Replaces the claude.ai session with `sessionKey` and `lastActiveOrg`, and drops the
    /// account-bound `routingHint` and `sessionKeyLC` rows (claude.ai issues them again).
    /// Throws when the database is absent or the write fails; Desktop must not be running.
    func writeSession(sessionKey: String, orgUuid: String) throws
}

enum ClaudeDesktopCookieError: Error, Equatable {
    /// The database is absent or SQLite refused a statement; nothing was committed.
    case sqlite(String)
    case encryptionFailed
}

/// `~/Library/Application Support/Claude/Cookies`, Chromium's cookie database encrypted with
/// the "Claude Safe Storage" password (`v10`, AES-128-CBC; meta version 24+ prefixes the
/// plaintext with sha256 of the host). Rows mirror what claude.ai sets in Desktop.
///
///     let cookies = ClaudeDesktopCookieFile(fileURL: ClaudeDesktopCookieFile.defaultURL, password: pw, now: Date.init)
///     try cookies.writeSession(sessionKey: account.sessionKey!, orgUuid: account.orgId!)
struct ClaudeDesktopCookieFile: ClaudeDesktopSessionWriting {
    static let defaultURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Claude/Cookies")

    private static let host = ".claude.ai"
    /// Long enough to outlive any switch; claude.ai renews the cookie on use.
    private static let lifetime: TimeInterval = 30 * 86400

    private let fileURL: URL
    private let password: String
    private let now: () -> Date

    /// All explicit so tests never reach the real file or Keychain.
    init(fileURL: URL, password: String, now: @escaping () -> Date) {
        self.fileURL = fileURL
        self.password = password
        self.now = now
    }

    func readSessionKey() -> String? {
        guard FileManager.default.fileExists(atPath: fileURL.path) else { return nil }
        return BrowserCookieService.readCookies(
            fromDatabaseAt: fileURL.path,
            encryptionKey: BrowserCookieService.deriveKey(from: password),
            now: now()).sessionKey
    }

    func writeSession(sessionKey: String, orgUuid: String) throws {
        guard FileManager.default.fileExists(atPath: fileURL.path) else {
            throw ClaudeDesktopCookieError.sqlite("no cookie database")
        }
        var db: OpaquePointer?
        guard sqlite3_open_v2(fileURL.path, &db, SQLITE_OPEN_READWRITE, nil) == SQLITE_OK else {
            sqlite3_close(db)
            throw ClaudeDesktopCookieError.sqlite("open")
        }
        defer { sqlite3_close(db) }
        sqlite3_busy_timeout(db, 2000)

        let prefixed = (Self.metaVersion(db) ?? 0) >= 24
        guard let sessionBlob = encrypt(sessionKey, prefixed: prefixed),
              let orgBlob = encrypt(orgUuid, prefixed: prefixed) else {
            throw ClaudeDesktopCookieError.encryptionFailed
        }
        let stamp = BrowserCookieService.chromeTime(now())
        let expires = BrowserCookieService.chromeTime(now().addingTimeInterval(Self.lifetime))

        try Self.exec(db, "BEGIN IMMEDIATE")
        do {
            try Self.exec(db, """
                DELETE FROM cookies WHERE host_key IN ('.claude.ai', 'claude.ai')
                AND name IN ('sessionKey', 'lastActiveOrg', 'routingHint', 'sessionKeyLC')
                """)
            // sessionKey: Secure, HttpOnly, Lax, set by the server; lastActiveOrg is script-readable.
            try Self.insert(db, name: "sessionKey", blob: sessionBlob, httpOnly: true, sourceType: 1,
                            stamp: stamp, expires: expires)
            try Self.insert(db, name: "lastActiveOrg", blob: orgBlob, httpOnly: false, sourceType: 2,
                            stamp: stamp, expires: expires)
            try Self.exec(db, "COMMIT")
        } catch {
            try? Self.exec(db, "ROLLBACK")
            throw error
        }
    }

    private func encrypt(_ value: String, prefixed: Bool) -> Data? {
        var plain = Data(value.utf8)
        if prefixed { plain = Data(SHA256.hash(data: Data(Self.host.utf8))) + plain }
        guard let cipher = BrowserCookieService.aes128CBC(
            plain, key: BrowserCookieService.deriveKey(from: password),
            iv: Data(repeating: 0x20, count: 16), encrypt: true) else { return nil }
        return Data("v10".utf8) + cipher
    }

    private static func metaVersion(_ db: OpaquePointer?) -> Int? {
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, "SELECT value FROM meta WHERE key = 'version'", -1, &stmt, nil) == SQLITE_OK
        else { return nil }
        defer { sqlite3_finalize(stmt) }
        guard sqlite3_step(stmt) == SQLITE_ROW, let text = sqlite3_column_text(stmt, 0) else { return nil }
        return Int(String(cString: text))
    }

    private static func exec(_ db: OpaquePointer?, _ sql: String) throws {
        guard sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK else {
            throw ClaudeDesktopCookieError.sqlite(String(cString: sqlite3_errmsg(db)))
        }
    }

    private static func insert(_ db: OpaquePointer?, name: String, blob: Data, httpOnly: Bool, sourceType: Int32,
                               stamp: Int64, expires: Int64) throws {
        let sql = """
            INSERT INTO cookies (creation_utc, host_key, top_frame_site_key, name, value, encrypted_value,
              path, expires_utc, is_secure, is_httponly, last_access_utc, has_expires, is_persistent,
              priority, samesite, source_scheme, source_port, last_update_utc, source_type,
              has_cross_site_ancestor)
            VALUES (?1, '.claude.ai', '', ?2, '', ?3, '/', ?4, 1, ?5, ?1, 1, 1, 1, 1, 2, 443, ?1, ?6, 1)
            """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else {
            throw ClaudeDesktopCookieError.sqlite(String(cString: sqlite3_errmsg(db)))
        }
        defer { sqlite3_finalize(stmt) }
        let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
        sqlite3_bind_int64(stmt, 1, stamp)
        sqlite3_bind_text(stmt, 2, name, -1, transient)
        _ = blob.withUnsafeBytes { sqlite3_bind_blob(stmt, 3, $0.baseAddress, Int32(blob.count), transient) }
        sqlite3_bind_int64(stmt, 4, expires)
        sqlite3_bind_int(stmt, 5, httpOnly ? 1 : 0)
        sqlite3_bind_int(stmt, 6, sourceType)
        guard sqlite3_step(stmt) == SQLITE_DONE else {
            throw ClaudeDesktopCookieError.sqlite(String(cString: sqlite3_errmsg(db)))
        }
    }
}
