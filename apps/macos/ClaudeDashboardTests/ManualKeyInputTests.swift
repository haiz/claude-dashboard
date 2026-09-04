import XCTest
@testable import ClaudeDashboard

/// A key arriving on stdin or from a clipboard almost always carries a newline.
final class ManualKeyInputTests: XCTestCase {

    func testTrimsATrailingNewline() {
        XCTAssertEqual(ManualKeyInput.trimmedKey(from: "sk-abc\n"), "sk-abc")
    }

    func testTrimsSurroundingWhitespace() {
        XCTAssertEqual(ManualKeyInput.trimmedKey(from: "  sk-abc  "), "sk-abc")
    }

    func testWhitespaceOnlyIsNoKey() {
        XCTAssertNil(ManualKeyInput.trimmedKey(from: " \n\t "))
    }

    func testEmptyIsNoKey() {
        XCTAssertNil(ManualKeyInput.trimmedKey(from: ""))
    }

    func testInteriorCharactersAreUntouched() {
        XCTAssertEqual(ManualKeyInput.trimmedKey(from: "sk-a.b-c_d\n"), "sk-a.b-c_d",
                       "no format guessing: only the edges are trimmed")
    }
}
