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
        kc.items[entryKey] = Data(#"{"claudeAiOauth":{"refreshToken":"old"},"mcpOAuth":{"figma":{"accessToken":"f"}}}"#.utf8)
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let new = try XCTUnwrap(OAuthCredential(json: Data(#"{"refreshToken":"new","expiresAt":5}"#.utf8)))

        try slot.writeOAuth(new)

        let root = try XCTUnwrap(OAuthAccountJSON.object(try XCTUnwrap(kc.items[entryKey])))
        XCTAssertEqual((root["mcpOAuth"] as? [String: Any])?.keys.sorted(), ["figma"])
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
}
