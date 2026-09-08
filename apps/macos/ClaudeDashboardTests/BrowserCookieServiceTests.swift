import CommonCrypto
import XCTest
@testable import ClaudeDashboard

final class BrowserCookieServiceTests: XCTestCase {

    func testParsesLocalStateWithBrowserTag() throws {
        let json = """
        {
          "profile": {
            "info_cache": {
              "Default": { "name": "Person 1", "user_name": "" },
              "Profile 1": { "name": "Work", "user_name": "work@example.com" },
              "Profile 2": { "name": "Personal", "user_name": "me@example.com" }
            }
          }
        }
        """.data(using: .utf8)!

        let profiles = BrowserCookieService.parseProfiles(from: json, browser: .brave)

        XCTAssertEqual(profiles.count, 3)
        XCTAssertEqual(profiles.first(where: { $0.path == "Profile 1" })?.displayName, "Work")
        XCTAssertEqual(profiles.first(where: { $0.path == "Profile 1" })?.googleEmail, "work@example.com")
        XCTAssertEqual(profiles.first(where: { $0.path == "Default" })?.googleEmail, "")
        XCTAssertTrue(profiles.allSatisfy { $0.browser == .brave })
    }

    func testPBKDF2KeyDerivation() throws {
        let key = BrowserCookieService.deriveKey(from: "test")
        XCTAssertEqual(key.count, 16)
        XCTAssertEqual(key, BrowserCookieService.deriveKey(from: "test"))
    }

    func testDecryptWithKnownValues() throws {
        let fakeEncrypted = Data([0x76, 0x31, 0x30]) + Data(repeating: 0, count: 32)
        let key = BrowserCookieService.deriveKey(from: "test")
        _ = BrowserCookieService.decryptCookieValue(fakeEncrypted, withKey: key)
    }

    func testFallbackProfilesFromDirectoryNames() throws {
        let names = ["Default", "Profile 1", "Profile 3", "GrShaderCache", "Local State", ".DS_Store"]
        let profiles = BrowserCookieService.profilesFromDirectoryNames(names, browser: .arc)

        XCTAssertEqual(profiles.map(\.path), ["Default", "Profile 1", "Profile 3"])
        XCTAssertTrue(profiles.allSatisfy { $0.browser == .arc })
        XCTAssertEqual(profiles.first?.displayName, "Default")
    }
}

// MARK: - Cookie row rules

/// Builds a Chromium cookie database on disk, so the row rules in
/// `readCookies` can be exercised without a browser.
private struct CookieFixture {
    let path: String
    private let key: Data

    init(key: Data) throws {
        self.key = key
        self.path = NSTemporaryDirectory()
            + "cookie-fixture-\(UUID().uuidString).db"
        try run(sql: """
            CREATE TABLE cookies (
                host_key TEXT, name TEXT, encrypted_value BLOB, expires_utc INTEGER
            );
            """)
    }

    /// `expiresUTC` is Chromium's own clock: microseconds since 1601-01-01,
    /// where 0 means a session cookie with no expiry at all.
    func insert(name: String, value: String, expiresUTC: Int64) throws {
        let blob = CookieFixture.encrypt(value, withKey: key)
        let hex = blob.map { String(format: "%02X", $0) }.joined()
        try run(sql: """
            INSERT INTO cookies VALUES ('.claude.ai', '\(name)', x'\(hex)', \(expiresUTC));
            """)
    }

    func remove() { try? FileManager.default.removeItem(atPath: path) }

    /// Chromium's v10 format: the literal "v10", then AES-128-CBC with a
    /// 16-space IV. The inverse of `decryptCookieValue`.
    private static func encrypt(_ value: String, withKey key: Data) -> Data {
        let plaintext = Data(value.utf8)
        let iv = Data(repeating: 0x20, count: 16)
        var out = Data(count: plaintext.count + kCCBlockSizeAES128)
        let capacity = out.count
        var written = 0
        let status = out.withUnsafeMutableBytes { outBytes in
            plaintext.withUnsafeBytes { plainBytes in
                key.withUnsafeBytes { keyBytes in
                    iv.withUnsafeBytes { ivBytes in
                        CCCrypt(
                            CCOperation(kCCEncrypt),
                            CCAlgorithm(kCCAlgorithmAES128),
                            CCOptions(kCCOptionPKCS7Padding),
                            keyBytes.baseAddress, key.count,
                            ivBytes.baseAddress,
                            plainBytes.baseAddress, plaintext.count,
                            outBytes.baseAddress, capacity,
                            &written
                        )
                    }
                }
            }
        }
        precondition(status == kCCSuccess, "fixture encryption failed")
        out.count = written
        return Data("v10".utf8) + out
    }

    private func run(sql: String) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/sqlite3")
        process.arguments = [path, sql]
        try process.run()
        process.waitUntilExit()
        precondition(process.terminationStatus == 0, "sqlite3 fixture failed")
    }
}

final class CookieRowRuleTests: XCTestCase {

    private let key = BrowserCookieService.deriveKey(from: "test")

    /// Chromium stores expiry as microseconds since 1601-01-01.
    private func chromeTime(_ date: Date) -> Int64 {
        Int64((date.timeIntervalSince1970 + 11_644_473_600) * 1_000_000)
    }

    func testLiveSessionKeyIsRead() throws {
        let fixture = try CookieFixture(key: key)
        defer { fixture.remove() }
        try fixture.insert(
            name: "sessionKey", value: "sk-ant-sid01-LIVE",
            expiresUTC: chromeTime(Date().addingTimeInterval(86_400)))

        let result = BrowserCookieService.readCookies(
            fromDatabaseAt: fixture.path, encryptionKey: key)

        XCTAssertEqual(result.sessionKey, "sk-ant-sid01-LIVE")
    }

    /// A key Chromium has kept past its expiry is dead: every request made
    /// with it is a guaranteed 401/403. Reading it turns a profile the user
    /// is simply logged out of into a validation failure, and the scan then
    /// blames the user's sign-in state for it.
    func testExpiredSessionKeyIsIgnored() throws {
        let fixture = try CookieFixture(key: key)
        defer { fixture.remove() }
        try fixture.insert(
            name: "sessionKey", value: "sk-ant-sid01-DEAD",
            expiresUTC: chromeTime(Date().addingTimeInterval(-86_400)))

        let result = BrowserCookieService.readCookies(
            fromDatabaseAt: fixture.path, encryptionKey: key)

        XCTAssertNil(result.sessionKey)
    }

    /// `expires_utc == 0` is Chromium for "session cookie, no expiry" — not
    /// "expired in 1601".
    func testSessionCookieWithoutExpiryIsRead() throws {
        let fixture = try CookieFixture(key: key)
        defer { fixture.remove() }
        try fixture.insert(
            name: "sessionKey", value: "sk-ant-sid01-NOEXPIRY", expiresUTC: 0)

        let result = BrowserCookieService.readCookies(
            fromDatabaseAt: fixture.path, encryptionKey: key)

        XCTAssertEqual(result.sessionKey, "sk-ant-sid01-NOEXPIRY")
    }
}
