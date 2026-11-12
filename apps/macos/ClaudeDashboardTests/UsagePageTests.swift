import XCTest
@testable import ClaudeDashboard

final class UsagePageTests: XCTestCase {

    private struct Failed: Error {}

    private func account(source: AccountSource, browser: Browser = .chrome, path: String = "Profile 2") -> Account {
        Account(id: UUID(), name: "a@x.com", email: "a@x.com", chromeProfilePath: path,
                browser: browser, plan: .pro, status: .active, source: source)
    }

    func testURLIsTheUsageSettingsPage() {
        XCTAssertEqual(UsagePage.url.absoluteString, "https://claude.ai/settings/usage")
    }

    func testScannedAccountTargetsItsOwnProfile() {
        XCTAssertEqual(UsagePage.target(for: account(source: .browser, browser: .brave)),
                       .profile(.brave, path: "Profile 2"))
    }

    func testPastedKeyTargetsTheDefaultBrowser() {
        XCTAssertEqual(UsagePage.target(for: account(source: .manual, path: "")), .defaultBrowser)
    }

    func testBrowserAccountWithoutProfileTargetsTheDefaultBrowser() {
        XCTAssertEqual(UsagePage.target(for: account(source: .browser, path: "")), .defaultBrowser)
    }

    func testOpenLaunchesTheProfileAndNotTheDefaultBrowser() {
        var commands: [BrowserProfileOpener.Command] = []
        var defaults: [URL] = []
        UsagePage.open(account(source: .browser),
                       opener: BrowserProfileOpener(run: { commands.append($0) }),
                       openDefault: { defaults.append($0) })
        XCTAssertEqual(commands, [BrowserProfileOpener.command(browser: .chrome, profilePath: "Profile 2", url: UsagePage.url)])
        XCTAssertEqual(defaults, [])
    }

    func testOpenFallsBackToTheDefaultBrowserWhenTheProfileCannotLaunch() {
        var defaults: [URL] = []
        UsagePage.open(account(source: .browser),
                       opener: BrowserProfileOpener(run: { _ in throw Failed() }),
                       openDefault: { defaults.append($0) })
        XCTAssertEqual(defaults, [UsagePage.url])
    }

    func testOpenUsesTheDefaultBrowserForAPastedKey() {
        var commands: [BrowserProfileOpener.Command] = []
        var defaults: [URL] = []
        UsagePage.open(account(source: .manual, path: ""),
                       opener: BrowserProfileOpener(run: { commands.append($0) }),
                       openDefault: { defaults.append($0) })
        XCTAssertEqual(commands, [])
        XCTAssertEqual(defaults, [UsagePage.url])
    }
}
