@testable import ClaudeDashboard
import XCTest

final class ClaudeDesktopVaultTests: XCTestCase {
    func testSaveLoadDeleteUnderDesktopService() throws {
        let kc = InMemoryKeychain()
        let vault = KeychainDesktopVault(keychain: kc)
        let id = UUID()
        let oauth = OAuthCredential(object: ["accessToken": "a", "refreshToken": "r",
                                             "expiresAt": 1_900_000_000_000])!
        let account = OAuthAccountJSON.canonical(["emailAddress": "a@b.co", "uuid": "u-1"])!
        let entry = VaultEntry(oauth: oauth, oauthAccount: account)

        XCTAssertEqual(KeychainDesktopVault.service, "ClaudeDashboard.desktop-vault")
        XCTAssertNotEqual(KeychainDesktopVault.service, KeychainCredentialVault.service)
        XCTAssertNil(try vault.load(id))

        try vault.save(entry, for: id)
        XCTAssertEqual(Array(kc.items.keys), ["ClaudeDashboard.desktop-vault|\(id.uuidString)"])
        XCTAssertEqual(try vault.load(id), entry)
        XCTAssertNil(try KeychainCredentialVault(keychain: kc).load(id))

        try vault.delete(id)
        XCTAssertNil(try vault.load(id))
    }
}
