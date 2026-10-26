import AppKit

/// Detects and quits Claude Desktop. Electron rewrites its login files on quit,
/// so the switcher writes only while `isRunning` is false.
protocol ClaudeDesktopProcessControlling {
    var isRunning: Bool { get }
    /// Asks Claude Desktop to quit, force-terminating after at most 5 s.
    /// Returns true when no Claude Desktop process remains.
    func quit() -> Bool
}

struct ClaudeDesktopProcess: ClaudeDesktopProcessControlling {
    static let bundleId = "com.anthropic.claudefordesktop"

    private static let pollInterval: TimeInterval = 0.1
    private static let gracePeriod: TimeInterval = 5

    private var apps: [NSRunningApplication] {
        NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleId)
    }

    var isRunning: Bool { !apps.isEmpty }

    func quit() -> Bool {
        guard isRunning else { return true }
        apps.forEach { $0.terminate() }
        if waitUntilGone(timeout: Self.gracePeriod) { return true }
        apps.forEach { $0.forceTerminate() }
        return waitUntilGone(timeout: 1)
    }

    private func waitUntilGone(timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while isRunning, Date() < deadline {
            Thread.sleep(forTimeInterval: Self.pollInterval)
        }
        return !isRunning
    }
}
