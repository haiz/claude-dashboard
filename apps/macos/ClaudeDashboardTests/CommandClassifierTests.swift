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

    // MARK: - Finding 1: ssh value-taking option arguments

    func testSshValueTakingOptionArgumentsAreNotMiscountedAsPositionals() {
        XCTAssertEqual(k("ssh -p 2222 host", "ssh -p 2222 host"), .interactive)
        XCTAssertEqual(k("ssh host", "ssh host"), .interactive)
        XCTAssertEqual(k("ssh host uptime", "ssh host uptime"), .nonInteractive)
        XCTAssertEqual(k("ssh -i key.pem host", "ssh -i key.pem host"), .interactive)
    }

    // MARK: - Finding 2: name-position matching, not argument prose

    func testArgumentProseDoesNotFalselyMatchInteractiveToolName() {
        // "more" appears only in quoted argument prose, never in the command name or
        // its shell expansion, so it must not be mistaken for the `more` pager.
        XCTAssertEqual(k("echo \"no more files\"", "echo"), .nonInteractive)
    }

    func testClaudeArgumentProseMentioningPrintFlagStaysInteractive() {
        // "-p" appears only inside quoted prose, not as an actual flag token, so print
        // mode must not be triggered.
        XCTAssertEqual(k("claude \"explain the -p flag to me\"", "claude"), .interactive)
    }

    func testClaudePrintModeStillDetectedAsRealFlag() {
        XCTAssertEqual(k("claude -p \"hi\"", "claude -p hi"), .nonInteractive)
    }

    // MARK: - Finding 3: leading-token allowlist before shell resolution

    func testResolverSkipsShellForNonAllowlistedLeadingToken() async {
        let start = Date()
        let result = await ShellCommandResolver().expand("$(echo pwned)")
        let elapsed = Date().timeIntervalSince(start)
        XCTAssertEqual(result, "", "a non-allowlisted leading token must never be interpolated into the shell script")
        XCTAssertLessThan(elapsed, 1.0, "non-allowlisted tokens must short-circuit before touching the shell")
    }

    // MARK: - Finding 4: resolver must never hang

    func testResolverDoesNotHangOnRealShell() async {
        let start = Date()
        let result = await ShellCommandResolver().expand("ls")
        let elapsed = Date().timeIntervalSince(start)
        XCTAssertLessThan(elapsed, 8.0, "ShellCommandResolver.expand must return within its timeout, never hang")
        _ = result // any string is acceptable; only completion within budget is asserted
    }
}
