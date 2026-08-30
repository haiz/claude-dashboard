import XCTest
@testable import ClaudeDashboard

final class AccountCodableTests: XCTestCase {

    // JSON cũ (trước multi-browser) không có key "browser" → phải decode về .chrome.
    func testDecodeLegacyJSONDefaultsToChrome() throws {
        let json = """
        {
          "id": "00000000-0000-0000-0000-000000000001",
          "name": "Work",
          "chromeProfilePath": "Profile 1",
          "plan": "Pro",
          "status": "active",
          "isPinned": false
        }
        """.data(using: .utf8)!

        let account = try JSONDecoder().decode(Account.self, from: json)
        XCTAssertEqual(account.browser, .chrome)
        XCTAssertEqual(account.chromeProfilePath, "Profile 1")
    }

    func testRoundTripPreservesBrowser() throws {
        let account = Account(
            id: UUID(),
            name: "Personal",
            email: "me@example.com",
            chromeProfilePath: "Default",
            chromeProfileName: "me@example.com",
            orgId: "org-1",
            sessionKey: "sk-1",
            browser: .brave,
            plan: .max5x,
            lastSynced: nil,
            status: .active
        )
        let data = try JSONEncoder().encode(account)
        let decoded = try JSONDecoder().decode(Account.self, from: data)
        XCTAssertEqual(decoded.browser, .brave)
        XCTAssertEqual(decoded, account)
    }
}
