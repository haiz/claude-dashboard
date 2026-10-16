import XCTest
@testable import ClaudeDashboard

final class AppVersionTests: XCTestCase {
    func testReturnsSemverStringFromInfoPlist() {
        let version = AppVersion.string
        XCTAssertTrue(
            version.range(of: #"^\d+\.\d+\.\d+$"#, options: .regularExpression) != nil,
            "Expected semver string (X.Y.Z), got '\(version)'"
        )
    }

    func testMatchesInfoPlistKey() {
        let expected = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        XCTAssertEqual(AppVersion.string, expected)
    }

    func testReleaseDateIsReadFromInfoPlist() {
        XCTAssertNotNil(AppVersion.releaseDate, "CDReleaseDate missing or not yyyy-MM-dd")
    }

    func testParseReleaseDate() {
        let date = AppVersion.parseReleaseDate("2026-10-16")
        XCTAssertEqual(date.map { Calendar.current.dateComponents([.year, .month, .day], from: $0) },
                       DateComponents(year: 2026, month: 10, day: 16))
        XCTAssertNil(AppVersion.parseReleaseDate("16/10/2026"))
    }
}
