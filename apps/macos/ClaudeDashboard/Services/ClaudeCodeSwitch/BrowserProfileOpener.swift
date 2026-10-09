import Foundation

/// Opens a URL in a specific browser profile, so the OAuth consent page loads under the
/// claude.ai session the dashboard scanned that account from.
///
/// Chrome, Brave and Edge take `--profile-directory=<name>`. Arc has no such switch, so
/// the URL opens in Arc and the user picks the space; this is the one browser where the
/// profile cannot be forced.
///
///     try BrowserProfileOpener().open(browser: account.browser,
///                                     profilePath: account.chromeProfilePath, url: authorizeURL)
struct BrowserProfileOpener {
    struct Command: Equatable {
        let launchPath: String
        let arguments: [String]
    }

    static func command(browser: Browser, profilePath: String, url: URL) -> Command {
        // `open -n -a <app> --args ...` launches a fresh instance with the given switches.
        switch browser {
        case .chrome, .brave, .edge:
            return Command(launchPath: "/usr/bin/open", arguments: [
                "-n", "-a", browser.displayName, "--args",
                "--profile-directory=\(profilePath)", url.absoluteString,
            ])
        case .arc:
            return Command(launchPath: "/usr/bin/open", arguments: ["-a", browser.displayName, url.absoluteString])
        }
    }

    /// Launches the browser. `run` is injectable so tests never spawn a process.
    var run: (Command) throws -> Void = { command in
        let process = Process()
        process.executableURL = URL(fileURLWithPath: command.launchPath)
        process.arguments = command.arguments
        try process.run()
    }

    func open(browser: Browser, profilePath: String, url: URL) throws {
        try run(Self.command(browser: browser, profilePath: profilePath, url: url))
    }
}
