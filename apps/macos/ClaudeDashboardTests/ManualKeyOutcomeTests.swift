import XCTest
@testable import ClaudeDashboard

/// The message mapping is the testable half of the paste sheet.
///
/// `ManualKeyOutcome`'s cases carry only an account `name: String`, or no payload
/// at all — never the session key itself. That is what makes a leak through
/// `.message` structurally impossible; there is no key value here for a switch
/// body to interpolate. A runtime check pattern-matching a key prefix (e.g.
/// `"sk-"`) cannot prove this and was removed: `ManualKeyInput` deliberately
/// avoids assuming a key format ("guessing at a prefix would break the day the
/// format changes"), so a check like that would pass even if a real key leaked
/// through in a different shape. The regression guard that can actually fail
/// lives in `DashboardViewModelTests` (`testManualKeyMessageNeverLeaksTheKeyOn...`),
/// which drives `applyManualKey` with a sentinel key and asserts on the sentinel.
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

    func testOnlyUpdatedWithNoChatOrgHoldsTheSheetOpen() {
        // Exhaustive switch, deliberately duplicating the enum's cases: adding a new
        // case to ManualKeyOutcome without updating this switch fails to compile, so
        // nobody can add a case to (or drop one from) the "holds open" set silently.
        func expectedToHoldOpen(_ outcome: ManualKeyOutcome) -> Bool {
            switch outcome {
            case .updatedWithNoChatOrg:
                return true
            case .added, .updated, .rejectedNoChatOrg, .keyNotAccepted, .emptyKey:
                return false
            }
        }

        let outcomes: [ManualKeyOutcome] = [
            .added(name: "x"), .updated(name: "x"), .updatedWithNoChatOrg(name: "x"),
            .rejectedNoChatOrg, .keyNotAccepted, .emptyKey
        ]
        for outcome in outcomes {
            XCTAssertEqual(outcome.holdsSheetOpen, expectedToHoldOpen(outcome),
                           "\(outcome) disagrees with the exhaustive expectation")
        }
    }
}
