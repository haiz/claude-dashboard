// ClaudeDashboardTests/CommandRunnerTests.swift
import XCTest
@testable import ClaudeDashboard

final class CommandRunnerTests: XCTestCase {
    var store: CommandLogStore!
    var dbPath: String!
    var runner: CommandRunner!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "test_runner_\(UUID().uuidString).db"
        store = await CommandLogStore(dbPath: dbPath)
        runner = CommandRunner(store: store)
    }

    override func tearDown() async throws {
        runner = nil
        store = nil
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    func testSuccessRecordsExitZero() async {
        let acct = UUID()
        let result = await runner.run(command: "echo hello", accountId: acct, trigger: .manual)
        XCTAssertEqual(result.status, .exited)
        XCTAssertEqual(result.exitCode, 0)

        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows[0].accountId, acct)
        XCTAssertEqual(rows[0].command, "echo hello")
        XCTAssertEqual(rows[0].status, .exited)
        XCTAssertEqual(rows[0].exitCode, 0)
        XCTAssertNotNil(rows[0].finishedAt)
    }

    func testNonZeroExitRecorded() async {
        let result = await runner.run(command: "exit 3", accountId: nil, trigger: .autoReset)
        XCTAssertEqual(result.exitCode, 3)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows[0].exitCode, 3)
        XCTAssertEqual(rows[0].status, .exited)
    }

    func testOnOutputReceivesTextAndTailStored() async {
        let box = OutputBox()
        let result = await runner.run(command: "echo streamed", accountId: nil, trigger: .manual) { chunk in
            box.append(chunk)
        }
        XCTAssertTrue(box.text.contains("streamed"), "got: \(box.text)")
        XCTAssertTrue(result.outputTail.contains("streamed"))
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows[0].output?.contains("streamed"), true)
    }

    func testTimeoutRecordsTimedOut() async {
        let fastRunner = CommandRunner(store: store, timeout: 1)
        let result = await fastRunner.run(command: "sleep 30", accountId: nil, trigger: .manual)
        XCTAssertEqual(result.status, .timedOut)
        XCTAssertNil(result.exitCode)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows[0].status, .timedOut)
    }

    func testTimeoutSigkillFallbackKillsStubbornProcess() async {
        // Timeout must exceed the time it takes zsh to source ~/.zshrc (a few seconds on a
        // loaded shell config), otherwise SIGTERM arrives before `trap` ever runs and the
        // fixture dies from the default disposition instead of exercising the SIGKILL path.
        let fastRunner = CommandRunner(store: store, timeout: 5)
        let start = Date()
        let result = await fastRunner.run(command: "trap '' TERM; while :; do sleep 0.2; done",
                                           accountId: nil, trigger: .manual)
        let elapsed = Date().timeIntervalSince(start)
        // Must survive past the SIGTERM (t=5s) and only die once the grace period's SIGKILL
        // lands (t=~7s) -- proving the fallback actually terminates a SIGTERM-ignoring tree.
        XCTAssertGreaterThanOrEqual(elapsed, 5, "expected process to survive the initial SIGTERM, died too early after \(elapsed)s")
        XCTAssertLessThan(elapsed, 10, "expected SIGKILL fallback to finish well under 10s, took \(elapsed)s")
        XCTAssertEqual(result.status, .timedOut)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows[0].status, .timedOut)
    }

    func testCancellationRecordsCancelled() async {
        let task = Task { await runner.run(command: "sleep 30", accountId: nil, trigger: .manual) }
        try? await Task.sleep(nanoseconds: 500_000_000)
        task.cancel()
        let result = await task.value
        XCTAssertEqual(result.status, .cancelled)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows[0].status, .cancelled)
    }
}

/// Thread-safe collector for streamed output in tests.
private final class OutputBox: @unchecked Sendable {
    private let lock = NSLock()
    private var buffer = ""
    var text: String { lock.lock(); defer { lock.unlock() }; return buffer }
    func append(_ s: String) { lock.lock(); buffer += s; lock.unlock() }
}
