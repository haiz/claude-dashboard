import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeSlotTests: XCTestCase {

    private let entryKey = "Claude Code-credentials|me"

    func testReadReturnsNilWhenEntryMissing() throws {
        let slot = KeychainClaudeCodeSlot(keychain: InMemoryKeychain(), account: "me")
        XCTAssertNil(try slot.readOAuth())
    }

    func testWriteReplacesOnlyClaudeAiOauthAndKeepsMcpOAuth() throws {
        let kc = InMemoryKeychain()
        kc.items[entryKey] = Data(#"{"claudeAiOauth":{"refreshToken":"old"},"mcpOAuth":{"figma":{"accessToken":"f"}},"other":{"k":1}}"#.utf8)
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let new = try XCTUnwrap(OAuthCredential(json: Data(#"{"refreshToken":"new","expiresAt":5}"#.utf8)))

        try slot.writeOAuth(new)

        let root = try XCTUnwrap(OAuthAccountJSON.object(try XCTUnwrap(kc.items[entryKey])))
        XCTAssertEqual((root["mcpOAuth"] as? [String: Any])?.keys.sorted(), ["figma"])
        let figma = (root["mcpOAuth"] as? [String: Any])?["figma"] as? [String: Any]
        XCTAssertEqual(figma?["accessToken"] as? String, "f")
        XCTAssertEqual((root["other"] as? [String: Any])?["k"] as? Int, 1)
        XCTAssertEqual(try slot.readOAuth(), new)
    }

    func testWriteCreatesEntryWhenMissing() throws {
        let kc = InMemoryKeychain()
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let c = try XCTUnwrap(OAuthCredential(json: Data(#"{"refreshToken":"r"}"#.utf8)))
        try slot.writeOAuth(c)
        XCTAssertEqual(try slot.readOAuth(), c)
    }

    func testVaultRoundTripKeyedByAccountId() throws {
        let kc = InMemoryKeychain()
        let vault = KeychainCredentialVault(keychain: kc)
        let id = UUID()
        let entry = VaultEntry(
            oauth: try XCTUnwrap(OAuthCredential(json: Data(#"{"refreshToken":"r"}"#.utf8))),
            oauthAccount: try XCTUnwrap(OAuthAccountJSON.canonical(["emailAddress": "a@b.co", "displayName": "Việt"])))

        XCTAssertNil(try vault.load(id))
        try vault.save(entry, for: id)

        XCTAssertNotNil(kc.items["ClaudeDashboard.cc-vault|\(id.uuidString)"])
        XCTAssertEqual(try vault.load(id), entry)
        XCTAssertNil(try vault.load(UUID()))
    }

    func testWriteThrowsAndKeepsBytesWhenEntryCorrupt() throws {
        let c = try XCTUnwrap(OAuthCredential(json: Data(#"{"refreshToken":"r"}"#.utf8)))
        for bytes in ["not json", "[1,2]"] {
            let kc = InMemoryKeychain()
            kc.items[entryKey] = Data(bytes.utf8)
            let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
            XCTAssertThrowsError(try slot.writeOAuth(c)) {
                XCTAssertEqual($0 as? ClaudeCodeSlotError, .unreadableEntry)
            }
            XCTAssertEqual(kc.items[entryKey], Data(bytes.utf8))
        }
    }

    func testReadThrowsWhenEntryCorrupt() {
        let kc = InMemoryKeychain()
        kc.items[entryKey] = Data("not json".utf8)
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        XCTAssertThrowsError(try slot.readOAuth()) {
            XCTAssertEqual($0 as? ClaudeCodeSlotError, .unreadableEntry)
        }
    }
}
