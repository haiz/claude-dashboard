import XCTest
@testable import ClaudeDashboard

final class ChromeCookieServiceTests: XCTestCase {

    func testParsesChromeLocalState() throws {
        let json = """
        {
          "profile": {
            "info_cache": {
              "Default": { "name": "Person 1" },
              "Profile 1": { "name": "Work" },
              "Profile 2": { "name": "Personal" }
            }
          }
        }
        """.data(using: .utf8)!

        let profiles = ChromeCookieService.parseProfiles(from: json)

        XCTAssertEqual(profiles.count, 3)
        XCTAssertEqual(profiles.first(where: { $0.path == "Default" })?.displayName, "Person 1")
        XCTAssertEqual(profiles.first(where: { $0.path == "Profile 1" })?.displayName, "Work")
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
