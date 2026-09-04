import XCTest
@testable import ClaudeDashboard

/// The message mapping is the testable half of the paste sheet.
final class ManualKeyOutcomeTests: XCTestCase {

    func testEachOutcomeHasAMessageThatNamesTheAccountWhenItHasOne() {
        XCTAssertTrue(ManualKeyOutcome.added(name: "a@b.com").message.contains("a@b.com"))
        XCTAssertTrue(ManualKeyOutcome.updated(name: "a@b.com").message.contains("a@b.com"))
        XCTAssertTrue(ManualKeyOutcome.updatedWithNoChatOrg(name: "a@b.com").message.contains("a@b.com"))
        XCTAssertFalse(ManualKeyOutcome.keyNotAccepted.message.isEmpty)
        XCTAssertFalse(ManualKeyOutcome.rejectedNoChatOrg.message.isEmpty)
        XCTAssertFalse(ManualKeyOutcome.emptyKey.message.isEmpty)
    }

    func testOnlyTheThreeFailureOutcomesAreFailures() {
        XCTAssertFalse(ManualKeyOutcome.added(name: "x").isFailure)
        XCTAssertFalse(ManualKeyOutcome.updated(name: "x").isFailure)
        XCTAssertFalse(ManualKeyOutcome.updatedWithNoChatOrg(name: "x").isFailure,
                       "the key was saved; the org is a warning, not a failure")
        XCTAssertTrue(ManualKeyOutcome.rejectedNoChatOrg.isFailure)
        XCTAssertTrue(ManualKeyOutcome.keyNotAccepted.isFailure)
        XCTAssertTrue(ManualKeyOutcome.emptyKey.isFailure)
    }

    func testNoMessageCanCarryTheKey() {
        // The mapping takes no key argument at all, which is the guarantee.
        let outcomes: [ManualKeyOutcome] = [
            .added(name: "a@b.com"), .updated(name: "a@b.com"),
            .updatedWithNoChatOrg(name: "a@b.com"), .rejectedNoChatOrg,
            .keyNotAccepted, .emptyKey
        ]
        for outcome in outcomes {
            XCTAssertFalse(outcome.message.lowercased().contains("sk-"),
                           "a session key must never reach a user-visible string")
        }
    }
}
