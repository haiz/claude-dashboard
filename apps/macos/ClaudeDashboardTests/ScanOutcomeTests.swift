import XCTest
@testable import ClaudeDashboard

final class ScanSummaryTests: XCTestCase {

    /// The list of offered accounts is the answer; a message on top of it
    /// would only compete with it.
    func testNoMessageWhenSomethingIsOffered() {
        XCTAssertNil(ScanSummary.message(
            offered: 1, skipped: [.sessionRejected(profile: "Profile 10")]))
    }

    func testNoMessageWhenNothingWasScanned() {
        XCTAssertNil(ScanSummary.message(offered: 0, skipped: []))
    }

    /// The regression this exists for: one long-dead key used to make
    /// "make sure you're signed in" the summary for profiles that were
    /// signed in and simply already added.
    func testEveryProfileGetsItsOwnReason() {
        let message = ScanSummary.message(offered: 0, skipped: [
            .alreadyAdded(profile: "Profile 1", account: "frontend@example.com"),
            .sessionRejected(profile: "Profile 10"),
            .noChatOrg(profile: "Profile 17"),
        ])

        XCTAssertEqual(message, """
        Nothing new to add. What the scan found:
        \u{2022} Profile 1: already added as frontend@example.com
        \u{2022} Profile 10: signed out — sign in to claude.ai in this profile
        \u{2022} Profile 17: no organization with chat access
        """)
    }

    /// "Profile 10" after "Profile 2", not before it.
    func testProfilesAreListedInHumanOrder() {
        let message = ScanSummary.message(offered: 0, skipped: [
            .unreachable(profile: "Profile 10"),
            .unreachable(profile: "Profile 2"),
        ])

        let profiles = (message ?? "").split(separator: "\n").dropFirst().map(String.init)
        XCTAssertEqual(profiles, [
            "\u{2022} Profile 2: could not be read",
            "\u{2022} Profile 10: could not be read",
        ])
    }
}
