@testable import ClaudeDashboard
import XCTest

final class ClaudeDesktopProcessTests: XCTestCase {
    func testBundleIdMatchesInstalledApp() throws {
        let plist = URL(fileURLWithPath: "/Applications/Claude.app/Contents/Info.plist")
        guard let info = NSDictionary(contentsOf: plist) else { return } // app absent (e.g. CI)
        XCTAssertEqual(info["CFBundleIdentifier"] as? String, ClaudeDesktopProcess.bundleId)
    }
}
