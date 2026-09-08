import Foundation

/// The `UserDefaults` the app reads and writes.
///
/// Not `.standard` unconditionally, because the `ClaudeDashboardTests` scheme
/// is hosted by `ClaudeDashboard.app`: every `xcodebuild test` launches the
/// real app, whose stores are built at startup against the developer's own
/// accounts. Any write in that path — the account store's quarantine move, its
/// pruning of dead run commands — then runs on a real machine, on data no test
/// declared. Diverting the host to a suite of its own is what makes a test run
/// observably harmless.
///
/// `CLAUDE_DASHBOARD_DEFAULTS_SUITE` is the same variable the helper CLI reads
/// (`HelperAccountStore.suiteVariable`), so a test driving both binaries can
/// point them at one suite.
enum AppDefaults {

    /// The environment variable that names the suite, for both binaries. It
    /// lives here rather than in the helper because `Helper/` is not in the
    /// app target; `HelperAccountStore.suiteVariable` re-exports it.
    static let suiteVariable = "CLAUDE_DASHBOARD_DEFAULTS_SUITE"

    /// The suite a test run is diverted to. Fixed rather than per-run: the
    /// point is to be somewhere other than the user's domain, and a stable
    /// name stays inspectable after a failure.
    static let testSuiteName = "com.claude-dashboard.app.tests"

    /// The suite to open, or nil to stay on the standard domain.
    ///
    /// Pure so the decision is testable; `shared` supplies the real inputs.
    static func suiteName(override: String?, isRunningTests: Bool) -> String? {
        if let override, !override.isEmpty { return override }
        return isRunningTests ? testSuiteName : nil
    }

    /// `getenv` rather than `ProcessInfo.processInfo.environment` for the same
    /// reason `HelperAccountStore` uses it: Foundation may serve a cached
    /// snapshot taken before a test called `setenv`.
    static func environmentOverride() -> String? {
        guard let raw = getenv(suiteVariable) else { return nil }
        return String(cString: raw)
    }

    /// Whether this process is running under XCTest. The variable is set by
    /// the test runner itself, so it is absent in every shipped launch.
    static func isRunningTests() -> Bool {
        getenv("XCTestConfigurationFilePath") != nil
            || getenv("XCTestSessionIdentifier") != nil
    }

    /// Resolved once: a store built later in the launch must not land in a
    /// different domain than one built earlier.
    static let shared: UserDefaults = {
        guard let name = suiteName(
            override: environmentOverride(), isRunningTests: isRunningTests()),
            let suite = UserDefaults(suiteName: name) else {
            return .standard
        }
        return suite
    }()
}
