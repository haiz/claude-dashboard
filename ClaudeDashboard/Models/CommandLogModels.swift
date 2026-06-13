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

/// One recorded command execution.
struct CommandLogEntry: Identifiable, Equatable {
    let id: Int64
    let accountId: UUID?        // nil if no account could be associated
    let command: String
    let trigger: CommandTrigger
    let startedAt: Date
    let finishedAt: Date?       // nil only if the process never reported termination
    let exitCode: Int32?        // nil if the process failed to launch
}
