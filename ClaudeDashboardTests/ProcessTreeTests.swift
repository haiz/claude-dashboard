import XCTest
import Darwin
@testable import ClaudeDashboard

final class ProcessTreeTests: XCTestCase {
    /// A lone `zsh -c "sleep 30"` tail-call-execs into sleep (same pid, no child), so
    /// the `&` forces a real forked child and `wait` keeps zsh alive as the parent;
    /// that gives killTree an actual descendant to reap.
    func testKillTreeReapsGrandchild() throws {
        let p = Process()
        p.launchPath = "/bin/zsh"
        p.arguments = ["-c", "sleep 30 & wait"]
        try p.run()
        let pid = p.processIdentifier

        // Give zsh a moment to spawn the sleep child.
        var kids: [Int32] = []
        for _ in 0..<50 {
            kids = ProcessTree.descendants(of: pid)
            if !kids.isEmpty { break }
            usleep(20_000)
        }
        XCTAssertFalse(kids.isEmpty, "expected a sleep descendant of the zsh process")
        let sleepPid = kids[0]

        ProcessTree.killTree(pid, signal: SIGKILL)

        // Wait for the descendant to actually die (kill(pid,0) -> -1/ESRCH).
        var reaped = false
        for _ in 0..<100 {
            if kill(sleepPid, 0) != 0 { reaped = true; break }
            usleep(20_000)
        }
        XCTAssertTrue(reaped, "grandchild sleep pid \(sleepPid) should be gone")
        p.waitUntilExit()
    }
}
