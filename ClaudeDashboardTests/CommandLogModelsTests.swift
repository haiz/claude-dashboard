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
}
