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
        let code = await runner.run(command: "echo hello", accountId: acct, trigger: .manual)
        XCTAssertEqual(code, 0)

        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows[0].accountId, acct)
        XCTAssertEqual(rows[0].command, "echo hello")
        XCTAssertEqual(rows[0].trigger, .manual)
        XCTAssertEqual(rows[0].exitCode, 0)
        XCTAssertNotNil(rows[0].finishedAt)
        XCTAssertGreaterThanOrEqual(rows[0].finishedAt!, rows[0].startedAt)
    }

    func testNonZeroExitRecorded() async {
        let code = await runner.run(command: "exit 3", accountId: nil, trigger: .autoReset)
        XCTAssertEqual(code, 3)
        let rows = await store.recent(limit: 10)
        XCTAssertEqual(rows[0].exitCode, 3)
        XCTAssertEqual(rows[0].trigger, .autoReset)
    }

    func testOnOutputReceivesText() async {
        let box = OutputBox()
        _ = await runner.run(command: "echo streamed", accountId: nil, trigger: .manual) { chunk in
            box.append(chunk)
        }
        XCTAssertTrue(box.text.contains("streamed"), "got: \(box.text)")
    }
}

/// Thread-safe collector for streamed output in tests.
private final class OutputBox: @unchecked Sendable {
    private let lock = NSLock()
    private var buffer = ""
    var text: String { lock.lock(); defer { lock.unlock() }; return buffer }
    func append(_ s: String) { lock.lock(); buffer += s; lock.unlock() }
}
