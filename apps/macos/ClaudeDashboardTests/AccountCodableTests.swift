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

    func testDecodesLegacyJSONWithoutAccountUuid() throws {
        let json = """
        [{"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"person@example.com",
          "chromeProfilePath":"Profile 1","orgId":"org-1","plan":"Max","status":"active"}]
        """.data(using: .utf8)!

        let accounts = try JSONDecoder().decode([Account].self, from: json)

        XCTAssertEqual(accounts.count, 1)
        XCTAssertNil(accounts[0].accountUuid)
    }

    func testRoundTripPreservesAccountUuid() throws {
        var account = Account(
            id: UUID(), name: "person@example.com", email: "person@example.com",
            chromeProfilePath: "Profile 1", chromeProfileName: "Profile 1",
            orgId: "org-1", sessionKey: nil, browser: .chrome, plan: .max200,
            lastSynced: nil, status: .active
        )
        account.accountUuid = "acct-1"

        let data = try JSONEncoder().encode([account])
        let decoded = try JSONDecoder().decode([Account].self, from: data)

        XCTAssertEqual(decoded[0].accountUuid, "acct-1")
    }
}
