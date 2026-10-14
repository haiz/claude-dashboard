import XCTest
@testable import ClaudeDashboard

final class PingMigrationTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!
    private var dir: URL!

    override func setUp() {
        super.setUp()
        suiteName = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suiteName)!
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("PingMigration-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suiteName)
        try? FileManager.default.removeItem(at: dir)
        super.tearDown()
    }

    private func touch(_ name: String) {
        FileManager.default.createFile(atPath: dir.appendingPathComponent(name).path, contents: Data("x".utf8))
    }

    private func exists(_ name: String) -> Bool {
        FileManager.default.fileExists(atPath: dir.appendingPathComponent(name).path)
    }

    func testRemovesTheCommandLogAndItsSidecars() {
        ["command_logs.db", "command_logs.db-wal", "command_logs.db-shm", "ping_logs.db", "usage_logs.db"].forEach(touch)

        PingMigration.run(defaults: defaults, logsDirectory: dir)

        XCTAssertFalse(exists("command_logs.db"))
        XCTAssertFalse(exists("command_logs.db-wal"))
        XCTAssertFalse(exists("command_logs.db-shm"))
        XCTAssertTrue(exists("ping_logs.db"))
        XCTAssertTrue(exists("usage_logs.db"))
    }

    func testRemovesRunCommandKeysAndKeepsTheRest() {
        let id = UUID().uuidString
        defaults.set("claude -p hi", forKey: "runCommand_\(id)")
        defaults.set(true, forKey: "runCommandTerminal_\(id)")
        defaults.set(true, forKey: "menuBarShowRunCommand")
        defaults.set(7, forKey: "autoRefreshMinutes")
        defaults.set(true, forKey: "autoSwitchEnabled")

        PingMigration.run(defaults: defaults, logsDirectory: dir)

        XCTAssertNil(defaults.object(forKey: "runCommand_\(id)"))
        XCTAssertNil(defaults.object(forKey: "runCommandTerminal_\(id)"))
        XCTAssertNil(defaults.object(forKey: "menuBarShowRunCommand"))
        XCTAssertEqual(defaults.integer(forKey: "autoRefreshMinutes"), 7)
        XCTAssertTrue(defaults.bool(forKey: "autoSwitchEnabled"))
    }

    func testDoesNotTurnAutoPingOn() {
        defaults.set("claude -p hi", forKey: "runCommand_\(UUID().uuidString)")

        PingMigration.run(defaults: defaults, logsDirectory: dir)

        XCTAssertNil(defaults.object(forKey: "autoPingEnabled"))
    }

    func testSecondRunIsANoOp() {
        touch("command_logs.db")
        defaults.set(true, forKey: "menuBarShowRunCommand")

        PingMigration.run(defaults: defaults, logsDirectory: dir)
        PingMigration.run(defaults: defaults, logsDirectory: dir)

        XCTAssertFalse(exists("command_logs.db"))
        XCTAssertNil(defaults.object(forKey: "menuBarShowRunCommand"))
    }
}
