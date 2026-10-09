import XCTest
@testable import ClaudeDashboard

final class BrowserProfileOpenerTests: XCTestCase {

    private let url = URL(string: "https://claude.com/cai/oauth/authorize?x=1")!

    func testChromeOpensTheNamedProfile() {
        let cmd = BrowserProfileOpener.command(browser: .chrome, profilePath: "Profile 1", url: url)
        XCTAssertEqual(cmd.launchPath, "/usr/bin/open")
        XCTAssertEqual(cmd.arguments,
            ["-n", "-a", "Google Chrome", "--args", "--profile-directory=Profile 1", url.absoluteString])
    }

    func testBraveAndEdgeUseTheirAppNames() {
        XCTAssertEqual(BrowserProfileOpener.command(browser: .brave, profilePath: "Default", url: url).arguments,
            ["-n", "-a", "Brave", "--args", "--profile-directory=Default", url.absoluteString])
        XCTAssertEqual(BrowserProfileOpener.command(browser: .edge, profilePath: "Default", url: url).arguments,
            ["-n", "-a", "Microsoft Edge", "--args", "--profile-directory=Default", url.absoluteString])
    }

    func testArcHasNoProfileDirectorySwitch() {
        // Arc has no --profile-directory; open the URL in Arc and let the user pick the space.
        let cmd = BrowserProfileOpener.command(browser: .arc, profilePath: "Profile 1", url: url)
        XCTAssertEqual(cmd.arguments, ["-a", "Arc", url.absoluteString])
    }
}
