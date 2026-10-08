import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeCredentialTests: XCTestCase {

    func testParsesDecisionFieldsAndKeepsUnknownOnes() throws {
        let json = Data("""
        {"accessToken":"a","refreshToken":"r1","expiresAt":1760000000000,
         "refreshTokenExpiresAt":1762000000000,"scopes":["user:inference"],"futureField":7}
        """.utf8)
        let c = try XCTUnwrap(OAuthCredential(json: json))
        XCTAssertEqual(c.refreshToken, "r1")
        XCTAssertEqual(c.expiresAt, Date(timeIntervalSince1970: 1_760_000_000))
        XCTAssertEqual(c.refreshTokenExpiresAt, Date(timeIntervalSince1970: 1_762_000_000))
        XCTAssertEqual(c.object["futureField"] as? Int, 7)
        XCTAssertFalse(c.isBlank)
    }

    func testBlankedCredentialIsBlankAndHasNoExpiry() throws {
        let c = try XCTUnwrap(OAuthCredential(json: Data(#"{"accessToken":"","refreshToken":"","expiresAt":0}"#.utf8)))
        XCTAssertTrue(c.isBlank)
        XCTAssertNil(c.expiresAt)
    }

    func testKeyOrderDoesNotAffectEquality() {
        let a = OAuthCredential(json: Data(#"{"refreshToken":"r","expiresAt":1}"#.utf8))
        let b = OAuthCredential(json: Data(#"{"expiresAt":1,"refreshToken":"r"}"#.utf8))
        XCTAssertEqual(a, b)
    }

    func testRejectsNonObject() {
        XCTAssertNil(OAuthCredential(json: Data("[1,2]".utf8)))
        XCTAssertNil(OAuthCredential(json: Data("not json".utf8)))
    }

    func testOAuthAccountEmail() {
        let json = OAuthAccountJSON.canonical(["emailAddress": "a@b.co", "displayName": "Việt"])!
        XCTAssertEqual(OAuthAccountJSON.email(json), "a@b.co")
        XCTAssertNil(OAuthAccountJSON.email(OAuthAccountJSON.canonical(["x": 1])!))
        XCTAssertNil(OAuthAccountJSON.canonical([1, 2]))
    }
}
