import XCTest
@testable import ClaudeDashboard

final class ClaudeDesktopSafeStorageTests: XCTestCase {
    func testEncryptDecryptRoundTrip() {
        let pw = "test-safe-storage-key-16"
        let plain = Data(#"{"oauth:tokenCacheV2":{}}"#.utf8)
        let blob = ClaudeDesktopSafeStorage.encrypt(plain, password: pw)!
        XCTAssertEqual(ClaudeDesktopSafeStorage.decrypt(blob, password: pw), plain)
    }

    func testEncryptEmitsV10() {
        let blob = ClaudeDesktopSafeStorage.encrypt(Data("x".utf8), password: "pw")!
        XCTAssertTrue(Data(base64Encoded: blob)!.prefix(3).elementsEqual(Array("v10".utf8)))
    }

    func testDecryptRejectsNonV10() {
        XCTAssertNil(ClaudeDesktopSafeStorage.decrypt(Data("not-v10".utf8).base64EncodedString(), password: "pw"))
    }
}
