import XCTest
import Darwin
@testable import ClaudeDashboard

final class RunningProcessRegistryTests: XCTestCase {
    func testAddRemoveCount() throws {
        let reg = RunningProcessRegistry()
        let p = Process()
        p.launchPath = "/bin/zsh"
        p.arguments = ["-c", "sleep 5"]
        try p.run()
        reg.add(p)
        XCTAssertEqual(reg.count, 1)
        reg.remove(pid: p.processIdentifier)
        XCTAssertEqual(reg.count, 0)
        ProcessTree.killTree(p.processIdentifier, signal: SIGKILL)
        p.waitUntilExit()
    }

    func testTerminateAllKillsTrackedProcess() throws {
        let reg = RunningProcessRegistry()
        let p = Process()
        p.launchPath = "/bin/zsh"
        p.arguments = ["-c", "sleep 30"]
        try p.run()
        reg.add(p)
        let pid = p.processIdentifier

        reg.terminateAll()

        var reaped = false
        for _ in 0..<100 {
            if kill(pid, 0) != 0 { reaped = true; break }
            usleep(20_000)
        }
        XCTAssertTrue(reaped, "terminateAll should kill the tracked process")
        p.waitUntilExit()
    }
}
