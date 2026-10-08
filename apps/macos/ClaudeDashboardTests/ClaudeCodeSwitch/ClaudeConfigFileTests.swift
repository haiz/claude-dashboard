import XCTest
@testable import ClaudeDashboard

final class ClaudeConfigFileTests: XCTestCase {

    private var url: URL!

    override func setUp() {
        super.setUp()
        url = FileManager.default.temporaryDirectory
            .appendingPathComponent("ClaudeConfigFileTests-\(UUID().uuidString).json")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: url)
        super.tearDown()
    }

    func testReadReturnsNilWhenFileOrKeyMissing() throws {
        XCTAssertNil(try ClaudeConfigFile(fileURL: url).readOAuthAccount())
        try Data(#"{"other":1}"#.utf8).write(to: url)
        XCTAssertNil(try ClaudeConfigFile(fileURL: url).readOAuthAccount())
    }

    func testReadReturnsCanonicalObject() throws {
        try Data(#"{"oauthAccount":{"organizationUuid":"o","emailAddress":"a@b.co"}}"#.utf8).write(to: url)
        let json = try XCTUnwrap(ClaudeConfigFile(fileURL: url).readOAuthAccount())
        XCTAssertEqual(json, OAuthAccountJSON.canonical(["emailAddress": "a@b.co", "organizationUuid": "o"]))
    }

    func testWritePreservesOtherKeysAndPermissions() throws {
        var root: [String: Any] = ["oauthAccount": ["emailAddress": "old@b.co"], "numStartups": 42]
        for i in 0..<300 { root["project\(i)"] = ["allowedTools": ["Bash"], "n": i] }
        try JSONSerialization.data(withJSONObject: root).write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o640], ofItemAtPath: url.path)
        let file = ClaudeConfigFile(fileURL: url)
        let newAccount = try XCTUnwrap(OAuthAccountJSON.canonical(["emailAddress": "new@b.co", "displayName": "Việt"]))

        try file.writeOAuthAccount(newAccount)

        let after = try XCTUnwrap(OAuthAccountJSON.object(Data(contentsOf: url)))
        XCTAssertEqual(after.count, root.count)
        XCTAssertEqual(after["numStartups"] as? Int, 42)
        XCTAssertEqual((after["project299"] as? [String: Any])?["n"] as? Int, 299)
        XCTAssertEqual(try file.readOAuthAccount(), newAccount)
        let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int
        XCTAssertEqual(mode, 0o640)
    }

    func testWriteRefusesUnreadableFile() throws {
        try Data("not json".utf8).write(to: url)
        let account = try XCTUnwrap(OAuthAccountJSON.canonical(["emailAddress": "a@b.co"]))
        XCTAssertThrowsError(try ClaudeConfigFile(fileURL: url).writeOAuthAccount(account)) {
            XCTAssertEqual($0 as? ClaudeConfigFileError, .unreadable)
        }
        XCTAssertEqual(try Data(contentsOf: url), Data("not json".utf8))
    }

    func testWriteKeepsSymlinkAndUpdatesTarget() throws {
        let link = url.deletingLastPathComponent().appendingPathComponent("link-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: link) }
        try Data(#"{"oauthAccount":{"emailAddress":"old@b.co"},"k":1}"#.utf8).write(to: url)
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: url)
        let account = try XCTUnwrap(OAuthAccountJSON.canonical(["emailAddress": "new@b.co"]))

        try ClaudeConfigFile(fileURL: link).writeOAuthAccount(account)

        XCTAssertEqual(try FileManager.default.attributesOfItem(atPath: link.path)[.type] as? FileAttributeType,
                       .typeSymbolicLink)
        XCTAssertNotNil(try? FileManager.default.destinationOfSymbolicLink(atPath: link.path))
        XCTAssertEqual(try ClaudeConfigFile(fileURL: url).readOAuthAccount(), account)
    }

    func testWritePreservesJSONValueTypes() throws {
        let root: [String: Any] = ["oauthAccount": ["emailAddress": "old@b.co"], "flagT": true, "flagF": false,
                                   "nothing": NSNull(), "ts": 1760000000000, "half": 0.5, "rate": 1.25,
                                   "url": "https://a/b"]
        try JSONSerialization.data(withJSONObject: root).write(to: url)
        let account = try XCTUnwrap(OAuthAccountJSON.canonical(["emailAddress": "new@b.co"]))

        try ClaudeConfigFile(fileURL: url).writeOAuthAccount(account)

        let after = try XCTUnwrap(OAuthAccountJSON.object(Data(contentsOf: url)))
        func isBool(_ x: Any?) -> Bool { x.map { CFGetTypeID($0 as CFTypeRef) == CFBooleanGetTypeID() } ?? false }
        XCTAssertTrue(isBool(after["flagT"])); XCTAssertEqual(after["flagT"] as? Bool, true)
        XCTAssertTrue(isBool(after["flagF"])); XCTAssertEqual(after["flagF"] as? Bool, false)
        XCTAssertTrue(after["nothing"] is NSNull)
        XCTAssertFalse(isBool(after["ts"])); XCTAssertEqual(after["ts"] as? Int, 1760000000000)
        XCTAssertEqual(after["half"] as? Double, 0.5)
        XCTAssertEqual(after["rate"] as? Double, 1.25)
        XCTAssertEqual(after["url"] as? String, "https://a/b")
    }
}
