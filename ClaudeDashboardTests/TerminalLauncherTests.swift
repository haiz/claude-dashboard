import XCTest
@testable import ClaudeDashboard

final class TerminalLauncherTests: XCTestCase {
    func testTerminalAppScriptContainsDoScript() {
        let s = TerminalLauncher.appleScript(command: "ccbf ping", preferITerm: false)
        XCTAssertTrue(s.contains("tell application \"Terminal\""))
        XCTAssertTrue(s.contains("do script \"ccbf ping\""))
        XCTAssertTrue(s.contains("activate"))
    }

    func testITermScriptTargetsITerm() {
        let s = TerminalLauncher.appleScript(command: "ccbf ping", preferITerm: true)
        XCTAssertTrue(s.contains("iTerm"))
        XCTAssertTrue(s.contains("ccbf ping"))
    }

    func testEscapesQuotesAndBackslashes() {
        let s = TerminalLauncher.appleScript(command: "echo \"a\\b\"", preferITerm: false)
        // Inner quotes and backslashes must be escaped for AppleScript string literals.
        XCTAssertTrue(s.contains("echo \\\"a\\\\b\\\""))
    }

    func testOpenUsesExecutor() throws {
        final class FakeExec: TerminalScriptExecutor, @unchecked Sendable {
            var last: String?
            func run(_ script: String) throws { last = script }
        }
        let fake = FakeExec()
        let launcher = TerminalLauncher(executor: fake, preferITerm: false)
        try launcher.open(command: "ccbf ping")
        XCTAssertTrue(fake.last?.contains("do script \"ccbf ping\"") == true)
    }

    func testOSAScriptExecutorThrowsOnInvalidScript() {
        XCTAssertThrowsError(try OSAScriptExecutor().run("§§§ this is not valid applescript §§§"))
    }
}
