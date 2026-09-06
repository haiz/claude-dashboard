import XCTest

/// Dispatch-level behaviour of the real binary: the two paths in `main.swift`
/// that run before any subcommand does.
///
/// The banner is asserted byte for byte because `contract/helper-cli.md` pins
/// it as shared text, not platform detail. It had drifted from the Linux
/// helper's wording without any test noticing: each side asserted only its own
/// string, so the pair could differ and both suites stay green. The Linux twin
/// of this file is `apps/linux/helper/tests/dispatch.rs`; a change to either
/// banner must redden both.
final class DispatchTests: XCTestCase {

    /// Byte-for-byte the banner in `contract/helper-cli.md`: seven lines, one
    /// trailing newline, no trailing blank line. The empty line before the
    /// closing delimiter *is* that trailing newline — Swift drops the break
    /// that precedes `"""`, so removing it would assert a banner with no
    /// newline at all.
    private static let usageBanner = """
    Usage: claude-dashboard-helper <command>

    Commands:
      decrypt    Decrypt accounts and output JSON to stdout
      sync       Scan installed browsers for Claude sessions and save to accounts
      usage      Fetch usage JSON for an account (args: <orgId> <sessionKey>)
      add-key    Add or repair one account from a session key on stdin

    """

    func testNoSubcommandPrintsTheUsageBannerAndExits1() {
        let result = HelperProcess.run([])

        XCTAssertEqual(result.stderr, Self.usageBanner)
        XCTAssertEqual(result.exitCode, 1)
        XCTAssertTrue(result.stdout.isEmpty, "the banner goes to stderr, never stdout")
    }

    func testUnknownSubcommandNamesItAndExits1() {
        let result = HelperProcess.run(["bogus"])

        XCTAssertEqual(result.stderr, "Unknown command: bogus\n")
        XCTAssertEqual(result.exitCode, 1)
        XCTAssertTrue(result.stdout.isEmpty)
    }
}
