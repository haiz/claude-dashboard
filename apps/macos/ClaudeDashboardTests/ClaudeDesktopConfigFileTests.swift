import XCTest
@testable import ClaudeDashboard

final class ClaudeDesktopConfigFileTests: XCTestCase {
    private let pw = "test-safe-storage-key-16"
    private var tmp: URL!

    override func setUpWithError() throws {
        tmp = FileManager.default.temporaryDirectory
            .appendingPathComponent("desktop-config-\(UUID().uuidString).json")
    }

    override func tearDownWithError() throws { try? FileManager.default.removeItem(at: tmp) }

    private var sample: OAuthCredential {
        OAuthCredential(object: [
            "accessToken": "sk-ant-oat01-AT", "refreshToken": "sk-ant-ort01-RT",
            "expiresAt": 1_900_000_000_000, "subscriptionType": "max", "rateLimitTier": "default_claude_max_5x",
        ])!
    }

    private func root() throws -> [String: Any] {
        OAuthAccountJSON.object(try Data(contentsOf: tmp))!
    }

    func testRoundTripGrant() throws {
        let cfg = ClaudeDesktopConfigFile(fileURL: tmp, password: pw)
        try cfg.writeGrant(sample, accountUuid: "AAA", orgUuid: "ORG")
        let got = try cfg.readActiveGrant()
        XCTAssertEqual(got?.accountUuid, "AAA")
        XCTAssertEqual(got?.oauth.object["accessToken"] as? String, "sk-ant-oat01-AT")
        XCTAssertEqual(got?.oauth.refreshToken, "sk-ant-ort01-RT")
        XCTAssertEqual(got?.oauth.expiresAt, Date(timeIntervalSince1970: 1_900_000_000))
        XCTAssertEqual(got?.oauth.object["subscriptionType"] as? String, "max")
        XCTAssertEqual(got?.oauth.object["rateLimitTier"] as? String, "default_claude_max_5x")
    }

    func testWriteDropsLegacyV1AndKeepsOtherKeys() throws {
        let seed = #"{"oauth:tokenCache":"legacy","theme":"dark","nested":{"a":1}}"#
        try Data(seed.utf8).write(to: tmp)
        try ClaudeDesktopConfigFile(fileURL: tmp, password: pw)
            .writeGrant(sample, accountUuid: "AAA", orgUuid: "ORG")
        let r = try root()
        XCTAssertNil(r["oauth:tokenCache"])
        XCTAssertEqual(r["theme"] as? String, "dark")
        XCTAssertEqual((r["nested"] as? [String: Any])?["a"] as? Int, 1)
        XCTAssertEqual(r["lastKnownAccountUuid"] as? String, "AAA")
    }

    func testWrittenMapKeyAndScope() throws {
        try ClaudeDesktopConfigFile(fileURL: tmp, password: pw)
            .writeGrant(sample, accountUuid: "AAA", orgUuid: "ORG")
        let plain = ClaudeDesktopSafeStorage.decrypt(try root()["oauth:tokenCacheV2"] as! String, password: pw)!
        let map = OAuthAccountJSON.object(plain)!
        XCTAssertEqual(Array(map.keys),
                       ["AAA:ORG:https://api.anthropic.com:" + ClaudeCodeOAuthClient.scope])
    }

    func testReadMissingConfigReturnsNil() throws {
        XCTAssertNil(try ClaudeDesktopConfigFile(fileURL: tmp, password: pw).readActiveGrant())
    }

    func testReadWithWrongPasswordReturnsNil() throws {
        try ClaudeDesktopConfigFile(fileURL: tmp, password: pw)
            .writeGrant(sample, accountUuid: "AAA", orgUuid: "ORG")
        XCTAssertNil(try ClaudeDesktopConfigFile(fileURL: tmp, password: "other-password-xx").readActiveGrant())
    }

    func testWriteToCorruptConfigThrowsAndLeavesFile() throws {
        try Data("not json".utf8).write(to: tmp)
        XCTAssertThrowsError(try ClaudeDesktopConfigFile(fileURL: tmp, password: pw)
            .writeGrant(sample, accountUuid: "A", orgUuid: "O")) {
            XCTAssertEqual($0 as? ClaudeDesktopConfigError, .unreadableConfig)
        }
        XCTAssertEqual(try Data(contentsOf: tmp), Data("not json".utf8))
    }
}
