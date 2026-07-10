import XCTest
@testable import ClaudeDashboard

final class CommandClassifierTests: XCTestCase {
    private func k(_ command: String, _ expanded: String) -> CommandKind {
        CommandClassifier.classify(command: command, expanded: expanded)
    }

    func testPlainNonInteractive() {
        XCTAssertEqual(k("ls -la", "/bin/ls -la"), .nonInteractive)
        XCTAssertEqual(k("echo hi", "echo hi"), .nonInteractive)
    }

    func testCcbfExpandsToInteractiveClaude() {
        let expanded = """
        ccbf () { claude-awake-f --model opusplan --enable-auto-mode --debug --verbose "$@" }
        claude-awake-f () { CLAUDE_CONFIG_DIR=~/.claude-frontend caffeinate -i /Users/x/.local/bin/claude "$@" }
        """
        XCTAssertEqual(k("ccbf ping", expanded), .interactive)
    }

    func testClaudePrintModeIsNonInteractive() {
        XCTAssertEqual(k("claude -p \"hi\"", "claude -p hi"), .nonInteractive)
        XCTAssertEqual(k("claude --print \"hi\"", "claude --print hi"), .nonInteractive)
    }

    func testClaudeInteractiveWhenNoPrintFlag() {
        XCTAssertEqual(k("claude \"hi\"", "claude hi"), .interactive)
    }

    func testEditorsAndPagersInteractive() {
        XCTAssertEqual(k("vim file.txt", "/usr/bin/vim file.txt"), .interactive)
        XCTAssertEqual(k("nano x", "nano x"), .interactive)
        XCTAssertEqual(k("top", "top"), .interactive)
        XCTAssertEqual(k("less big.log", "less big.log"), .interactive)
    }

    func testSshInteractiveOnlyWithoutRemoteCommand() {
        XCTAssertEqual(k("ssh host", "ssh host"), .interactive)
        XCTAssertEqual(k("ssh host uptime", "ssh host uptime"), .nonInteractive)
    }
}
