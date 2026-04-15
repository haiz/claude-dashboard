import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeAccountDetectorTests: XCTestCase {

    private var tempDir: URL!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("ClaudeCodeAccountDetectorTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    private func writeClaudeJson(_ body: String) throws -> URL {
        let url = tempDir.appendingPathComponent(".claude.json")
        try body.write(to: url, atomically: true, encoding: .utf8)
        return url
    }

    func testReturnsEmail_whenValidJson() throws {
        let url = try writeClaudeJson("""
        {
          "oauthAccount": {
            "organizationUuid": "d9687f13-3bbe-4738-b68f-454c2171cc5d",
            "emailAddress": "test@example.com"
          },
          "unrelated": 42
        }
        """)
        let detector = ClaudeCodeAccountDetector(fileURL: url)
        XCTAssertEqual(detector.activeEmail(), "test@example.com")
    }

    func testReturnsNil_whenFileMissing() {
        let url = tempDir.appendingPathComponent("nonexistent.json")
        let detector = ClaudeCodeAccountDetector(fileURL: url)
        XCTAssertNil(detector.activeEmail())
    }

    func testReturnsNil_whenMalformedJson() throws {
        let url = try writeClaudeJson("this is not json {")
        let detector = ClaudeCodeAccountDetector(fileURL: url)
        XCTAssertNil(detector.activeEmail())
    }

    func testReturnsNil_whenOauthAccountAbsent() throws {
        let url = try writeClaudeJson("""
        { "something": "else" }
        """)
        let detector = ClaudeCodeAccountDetector(fileURL: url)
        XCTAssertNil(detector.activeEmail())
    }

    func testReturnsNil_whenEmailAddressAbsent() throws {
        let url = try writeClaudeJson("""
        { "oauthAccount": { "organizationUuid": "abc-123" } }
        """)
        let detector = ClaudeCodeAccountDetector(fileURL: url)
        XCTAssertNil(detector.activeEmail())
    }
}
