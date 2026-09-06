import XCTest

/// The other half of the harness. `HelperProcess` spawns a real binary, so its
/// failure modes are process-level: a mistake here does not fail a test, it
/// takes the whole bundle down.
final class HelperProcessTests: XCTestCase {

    /// Writing stdin to a helper that never reads it must not kill the runner.
    ///
    /// `add-key`'s argument-rejection paths (Task 9) exit before reading stdin.
    /// A payload larger than the 64KB pipe buffer cannot be absorbed by the
    /// buffer alone, so the write runs into the dead reader and gets EPIPE —
    /// and EPIPE raises SIGPIPE, whose disposition in the xctest process was
    /// measured to be SIG_DFL. Both `FileHandle.write(_:)` and
    /// `write(contentsOf:)` were measured killing the runner here; the fix is
    /// `F_SETNOSIGPIPE` on the write end, which turns it into a thrown EPIPE.
    ///
    /// `usage` with one argument is a helper that exits immediately without
    /// reading stdin, so it stands in for `add-key`'s rejection paths without
    /// depending on anything Task 9 has not written yet.
    func testStdinForAHelperThatNeverReadsItDoesNotKillTheRunner() {
        let result = HelperProcess.run(["usage"], stdin: String(repeating: "x", count: 256 * 1024))

        // Reaching this line at all is the assertion: an unguarded write would
        // have taken the process down before `run` returned.
        XCTAssertEqual(result.exitCode, 1)
        XCTAssertFalse(result.stderr.isEmpty, "the arg-count rejection writes to stderr")
    }
}
