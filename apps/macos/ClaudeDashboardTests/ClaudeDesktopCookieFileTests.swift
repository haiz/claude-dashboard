@testable import ClaudeDashboard
import CryptoKit
import XCTest

/// `ClaudeDesktopCookieFile` against a Chromium cookie database with Claude Desktop's real
/// schema (meta version 24), built with `/usr/bin/sqlite3`.
final class ClaudeDesktopCookieFileTests: XCTestCase {
    private let password = "test-pw"
    private var url: URL!
    private var file: ClaudeDesktopCookieFile!

    override func setUp() {
        url = FileManager.default.temporaryDirectory
            .appendingPathComponent("desktop-cookies-\(UUID().uuidString).db")
        file = ClaudeDesktopCookieFile(fileURL: url, password: password,
                                       now: { Date(timeIntervalSince1970: 1_800_000_000) })
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: url)
    }

    private func sql(_ statement: String) throws -> String {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/sqlite3")
        process.arguments = [url.path, statement]
        process.standardOutput = pipe
        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, statement)
        let out = pipe.fileHandleForReading.readDataToEndOfFile()
        return String(decoding: out, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func makeDatabase() throws {
        _ = try sql("""
            CREATE TABLE meta(key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR);
            INSERT INTO meta VALUES ('version', '24'), ('last_compatible_version', '24');
            CREATE TABLE cookies(creation_utc INTEGER NOT NULL,host_key TEXT NOT NULL,top_frame_site_key TEXT NOT NULL,name TEXT NOT NULL,value TEXT NOT NULL,encrypted_value BLOB NOT NULL,path TEXT NOT NULL,expires_utc INTEGER NOT NULL,is_secure INTEGER NOT NULL,is_httponly INTEGER NOT NULL,last_access_utc INTEGER NOT NULL,has_expires INTEGER NOT NULL,is_persistent INTEGER NOT NULL,priority INTEGER NOT NULL,samesite INTEGER NOT NULL,source_scheme INTEGER NOT NULL,source_port INTEGER NOT NULL,last_update_utc INTEGER NOT NULL,source_type INTEGER NOT NULL,has_cross_site_ancestor INTEGER NOT NULL);
            CREATE UNIQUE INDEX cookies_unique_index ON cookies(host_key, top_frame_site_key, has_cross_site_ancestor, name, path, source_scheme, source_port);
            INSERT INTO cookies VALUES (1,'.claude.ai','','sessionKey','',x'00','/',1,1,1,1,1,1,1,1,2,443,1,1,1);
            INSERT INTO cookies VALUES (1,'.claude.ai','','lastActiveOrg','',x'00','/',1,1,0,1,1,1,1,1,2,443,1,2,1);
            INSERT INTO cookies VALUES (1,'.claude.ai','','routingHint','',x'00','/',1,1,1,1,1,1,1,1,2,443,1,1,1);
            INSERT INTO cookies VALUES (1,'.claude.ai','','sessionKeyLC','',x'00','/',1,1,0,1,1,1,1,1,2,443,1,1,1);
            INSERT INTO cookies VALUES (1,'.claude.ai','','__cf_bm','',x'00','/',1,1,1,1,1,1,1,0,2,443,1,1,1);
            """)
    }

    func testWriteSessionReplacesTheClaudeSession() throws {
        try makeDatabase()
        try file.writeSession(sessionKey: "sk-ant-new", orgUuid: "org-new")

        let key = BrowserCookieService.deriveKey(from: password)
        let read = BrowserCookieService.readCookies(fromDatabaseAt: url.path, encryptionKey: key,
                                                    now: Date(timeIntervalSince1970: 1_800_000_000))
        XCTAssertEqual(read.sessionKey, "sk-ant-new")
        XCTAssertEqual(read.orgId, "org-new")
        XCTAssertEqual(file.readSessionKey(), "sk-ant-new")
        // Account-bound rows go; unrelated rows stay.
        XCTAssertEqual(try sql("SELECT name FROM cookies ORDER BY name"), "__cf_bm\nlastActiveOrg\nsessionKey")
        XCTAssertEqual(try sql("SELECT is_httponly, samesite, has_cross_site_ancestor, is_persistent FROM cookies WHERE name='sessionKey'"),
                       "1|1|1|1")
    }

    func testVersion24ValueCarriesTheHostDigest() throws {
        try makeDatabase()
        try file.writeSession(sessionKey: "sk-ant-new", orgUuid: "org-new")
        let hex = try sql("SELECT hex(encrypted_value) FROM cookies WHERE name='sessionKey'")
        var blob = Data()
        var index = hex.startIndex
        while index < hex.endIndex {
            let next = hex.index(index, offsetBy: 2)
            blob.append(UInt8(hex[index..<next], radix: 16)!)
            index = next
        }
        XCTAssertEqual(blob.prefix(3), Data("v10".utf8))
        let plain = try XCTUnwrap(BrowserCookieService.aes128CBC(
            Data(blob.dropFirst(3)), key: BrowserCookieService.deriveKey(from: password),
            iv: Data(repeating: 0x20, count: 16), encrypt: false))
        XCTAssertEqual(plain.prefix(32), Data(SHA256.hash(data: Data(".claude.ai".utf8))))
        XCTAssertEqual(String(decoding: plain.dropFirst(32), as: UTF8.self), "sk-ant-new")
    }

    func testMissingDatabase() {
        XCTAssertNil(file.readSessionKey())
        XCTAssertThrowsError(try file.writeSession(sessionKey: "sk", orgUuid: "org"))
    }
}
