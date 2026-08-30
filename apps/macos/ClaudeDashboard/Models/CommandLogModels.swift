// ClaudeDashboard/Models/CommandLogModels.swift
import Foundation

/// Why a logged command ran.
enum CommandTrigger: Int, CaseIterable {
    case manual = 0      // user clicked Run in RunCommandSheet
    case autoReset = 1   // a usage window reset was detected
    case autoEmpty = 2   // a refresh produced no 5h/7d usage

    var label: String {
        switch self {
        case .manual: return "Manual"
        case .autoReset: return "Auto (reset)"
        case .autoEmpty: return "Auto (empty)"
        }
    }
}

/// Terminal outcome of a command execution.
enum CommandStatus: Int, CaseIterable {
    case exited = 0             // process ran to completion (see exitCode)
    case timedOut = 1           // killed after exceeding the run timeout
    case cancelled = 2          // killed because the user cancelled / dismissed
    case launchedInTerminal = 3 // handed off to Terminal/iTerm, not tracked further
    case launchFailed = 4       // process/terminal failed to launch

    var label: String {
        switch self {
        case .exited: return "Exited"
        case .timedOut: return "Timed out"
        case .cancelled: return "Cancelled"
        case .launchedInTerminal: return "In Terminal"
        case .launchFailed: return "Launch failed"
        }
    }
}

/// Outcome returned by CommandRunner.
struct CommandResult: Sendable, Equatable {
    let status: CommandStatus
    let exitCode: Int32?     // set only when status == .exited
    let outputTail: String   // bounded tail of stdout+stderr
}

/// One recorded command execution.
struct CommandLogEntry: Identifiable, Equatable {
    let id: Int64
    let accountId: UUID?        // nil if no account could be associated
    let command: String
    let trigger: CommandTrigger
    let startedAt: Date
    let finishedAt: Date?       // nil only if the process never reported termination
    let exitCode: Int32?        // nil if the process failed to launch
    let status: CommandStatus   // historical rows (pre-migration) decode as .exited
    let output: String?         // nil when no output captured
}
