import XCTest
@testable import ClaudeDashboard

final class BrowserTests: XCTestCase {

    func testAllCasesPresent() {
        XCTAssertEqual(Set(Browser.allCases), [.chrome, .arc, .brave, .edge])
    }

    func testKeychainServiceNames() {
        XCTAssertEqual(Browser.chrome.keychainService, "Chrome Safe Storage")
        XCTAssertEqual(Browser.arc.keychainService, "Arc Safe Storage")
        XCTAssertEqual(Browser.brave.keychainService, "Brave Safe Storage")
        XCTAssertEqual(Browser.edge.keychainService, "Microsoft Edge Safe Storage")
    }

    func testBasePathSuffixes() {
        let home = NSHomeDirectory() + "/Library/Application Support/"
        XCTAssertEqual(Browser.chrome.basePath, home + "Google/Chrome")
        XCTAssertEqual(Browser.arc.basePath, home + "Arc/User Data")
        XCTAssertEqual(Browser.brave.basePath, home + "BraveSoftware/Brave-Browser")
        XCTAssertEqual(Browser.edge.basePath, home + "Microsoft Edge")
    }

    func testDisplayNames() {
        XCTAssertEqual(Browser.chrome.displayName, "Google Chrome")
        XCTAssertEqual(Browser.arc.displayName, "Arc")
        XCTAssertEqual(Browser.brave.displayName, "Brave")
        XCTAssertEqual(Browser.edge.displayName, "Microsoft Edge")
    }

    func testRawValueCodable() throws {
        let data = try JSONEncoder().encode(Browser.brave)
        XCTAssertEqual(String(data: data, encoding: .utf8), "\"brave\"")
        let decoded = try JSONDecoder().decode(Browser.self, from: "\"edge\"".data(using: .utf8)!)
        XCTAssertEqual(decoded, .edge)
    }
}
