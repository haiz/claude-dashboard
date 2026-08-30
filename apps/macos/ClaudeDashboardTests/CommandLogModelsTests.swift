// ClaudeDashboardTests/CommandLogModelsTests.swift
import XCTest
@testable import ClaudeDashboard

final class CommandLogModelsTests: XCTestCase {
    func testTriggerLabels() {
        XCTAssertEqual(CommandTrigger.manual.label, "Manual")
        XCTAssertEqual(CommandTrigger.autoReset.label, "Auto (reset)")
        XCTAssertEqual(CommandTrigger.autoEmpty.label, "Auto (empty)")
    }

    func testTriggerRawValuesStable() {
        XCTAssertEqual(CommandTrigger.manual.rawValue, 0)
        XCTAssertEqual(CommandTrigger.autoReset.rawValue, 1)
        XCTAssertEqual(CommandTrigger.autoEmpty.rawValue, 2)
    }

    func testStatusRawValuesStable() {
        XCTAssertEqual(CommandStatus.exited.rawValue, 0)
        XCTAssertEqual(CommandStatus.timedOut.rawValue, 1)
        XCTAssertEqual(CommandStatus.cancelled.rawValue, 2)
        XCTAssertEqual(CommandStatus.launchedInTerminal.rawValue, 3)
        XCTAssertEqual(CommandStatus.launchFailed.rawValue, 4)
    }

    func testStatusLabels() {
        XCTAssertEqual(CommandStatus.exited.label, "Exited")
        XCTAssertEqual(CommandStatus.timedOut.label, "Timed out")
        XCTAssertEqual(CommandStatus.cancelled.label, "Cancelled")
        XCTAssertEqual(CommandStatus.launchedInTerminal.label, "In Terminal")
        XCTAssertEqual(CommandStatus.launchFailed.label, "Launch failed")
    }
}
