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
