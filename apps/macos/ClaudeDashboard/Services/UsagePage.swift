import AppKit

/// Opens claude.ai's usage page for one account, in the browser profile the account was
/// scanned from, so the page loads signed in as that account (the default browser could
/// show another one). A pasted key has no profile behind it and opens in the default
/// browser; so does a profile whose browser cannot be launched. Arc opens in Arc, where the
/// user picks the space (`BrowserProfileOpener`).
///
///     UsagePage.open(state.account)
enum UsagePage {
    static let url = URL(string: "https://claude.ai/settings/usage")!

    enum Target: Equatable {
        case profile(Browser, path: String)
        case defaultBrowser
    }

    static func target(for account: Account) -> Target {
        guard account.source == .browser, !account.chromeProfilePath.isEmpty else { return .defaultBrowser }
        return .profile(account.browser, path: account.chromeProfilePath)
    }

    /// `opener` and `openDefault` are injectable so tests never launch a browser.
    static func open(_ account: Account,
                     opener: BrowserProfileOpener = BrowserProfileOpener(),
                     openDefault: (URL) -> Void = { NSWorkspace.shared.open($0) }) {
        if case .profile(let browser, let path) = target(for: account),
           (try? opener.open(browser: browser, profilePath: path, url: url)) != nil {
            return
        }
        openDefault(url)
    }
}
