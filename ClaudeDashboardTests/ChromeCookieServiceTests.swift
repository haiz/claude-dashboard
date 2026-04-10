import XCTest
@testable import ClaudeDashboard

final class ChromeCookieServiceTests: XCTestCase {

    func testParsesChromeLocalState() throws {
        let json = """
        {
          "profile": {
            "last_active_profiles": ["Default", "Profile 1"],
            "info_cache": {
              "Default": { "name": "Person 1", "user_name": "" },
              "Profile 1": { "name": "Work", "user_name": "work@example.com" },
              "Profile 2": { "name": "Personal", "user_name": "me@example.com" }
            }
          }
        }
        """.data(using: .utf8)!

        let profiles = ChromeCookieService.parseProfiles(from: json)

        // Only active profiles (Default + Profile 1), not Profile 2
        XCTAssertEqual(profiles.count, 2)
        XCTAssertEqual(profiles.first(where: { $0.path == "Default" })?.displayName, "Person 1")
        XCTAssertEqual(profiles.first(where: { $0.path == "Profile 1" })?.displayName, "Work")
        XCTAssertEqual(profiles.first(where: { $0.path == "Profile 1" })?.googleEmail, "work@example.com")
        XCTAssertEqual(profiles.first(where: { $0.path == "Default" })?.googleEmail, "")
        XCTAssertNil(profiles.first(where: { $0.path == "Profile 2" }))
    }

    func testPBKDF2KeyDerivation() throws {
        let key = ChromeCookieService.deriveKey(from: "test")
        XCTAssertEqual(key.count, 16)
        let key2 = ChromeCookieService.deriveKey(from: "test")
        XCTAssertEqual(key, key2)
    }

    func testDecryptWithKnownValues() throws {
        let fakeEncrypted = Data([0x76, 0x31, 0x30]) + Data(repeating: 0, count: 32)
        let key = ChromeCookieService.deriveKey(from: "test")
        let _ = ChromeCookieService.decryptCookieValue(fakeEncrypted, withKey: key)
    }
}
