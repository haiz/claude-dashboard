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
}
