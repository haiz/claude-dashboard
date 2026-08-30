// ClaudeDashboard/Services/TerminalLauncher.swift
import Foundation

/// Runs an AppleScript. Injectable so tests assert the script without launching a terminal.
protocol TerminalScriptExecutor: Sendable {
    func run(_ script: String) throws
}

struct OSAScriptExecutor: TerminalScriptExecutor {
    func run(_ script: String) throws {
        let p = Process()
        p.launchPath = "/usr/bin/osascript"
        p.arguments = ["-e", script]
        let errorPipe = Pipe()
        p.standardError = errorPipe
        try p.run()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else {
            let errorData = errorPipe.fileHandleForReading.readDataToEndOfFile()
            let stderrText = String(data: errorData, encoding: .utf8)?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let message = stderrText.isEmpty
                ? "osascript exited with status \(p.terminationStatus)"
                : stderrText
            throw NSError(domain: "TerminalLauncher", code: Int(p.terminationStatus),
                           userInfo: [NSLocalizedDescriptionKey: message])
        }
    }
}

/// Opens an interactive terminal (iTerm if installed, else Terminal.app) running the
/// command. The terminal starts the user's login+interactive shell, so ~/.zshrc
/// functions like `ccbf` resolve and there is a real TTY.
struct TerminalLauncher: Sendable {
    let executor: TerminalScriptExecutor
    let preferITerm: Bool

    init(executor: TerminalScriptExecutor = OSAScriptExecutor(),
         preferITerm: Bool = TerminalLauncher.itermInstalled()) {
        self.executor = executor
        self.preferITerm = preferITerm
    }

    func open(command: String) throws {
        try executor.run(Self.appleScript(command: command, preferITerm: preferITerm))
    }

    static func itermInstalled() -> Bool {
        FileManager.default.fileExists(atPath: "/Applications/iTerm.app")
    }

    /// Escape a Swift string for embedding inside an AppleScript double-quoted literal.
    private static func escape(_ s: String) -> String {
        s.replacingOccurrences(of: "\\", with: "\\\\")
         .replacingOccurrences(of: "\"", with: "\\\"")
    }

    static func appleScript(command: String, preferITerm: Bool) -> String {
        let cmd = escape(command)
        if preferITerm {
            return """
            tell application "iTerm"
                activate
                set w to (create window with default profile)
                tell current session of w to write text "\(cmd)"
            end tell
            """
        }
        return """
        tell application "Terminal"
            activate
            do script "\(cmd)"
        end tell
        """
    }
}
