// ClaudeDashboard/Services/CommandRunner.swift
import Foundation

/// The single place the app launches shell commands. Wraps `Process`, captures
/// timing + exit code, and writes one row to `CommandLogStore`.
struct CommandRunner: Sendable {
    let store: CommandLogStore

    /// Runs `command` via `/bin/zsh -c`, waits for it to exit, then records a log row.
    /// - Parameter onOutput: optional callback fed decoded stdout/stderr chunks for
    ///   live display. Called on a background queue.
    /// - Returns: the process exit code, or nil if the process failed to launch.
    @discardableResult
    func run(command: String,
             accountId: UUID?,
             trigger: CommandTrigger,
             onOutput: (@Sendable (String) -> Void)? = nil) async -> Int32? {
        let startedAt = Date()
        let process = Process()
        process.launchPath = "/bin/zsh"
        process.arguments = ["-c", command]
        process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())

        let outPipe = Pipe()
        let errPipe = Pipe()
        process.standardOutput = outPipe
        process.standardError = errPipe

        if let onOutput {
            let handle: @Sendable (Data) -> Void = { data in
                guard !data.isEmpty, let s = String(data: data, encoding: .utf8) else { return }
                onOutput(s)
            }
            outPipe.fileHandleForReading.readabilityHandler = { handle($0.availableData) }
            errPipe.fileHandleForReading.readabilityHandler = { handle($0.availableData) }
        }

        // Set terminationHandler BEFORE run() to avoid a fast-exit race. If run()
        // throws, the handler never fires, so resume with launched=false there.
        let launched: Bool = await withCheckedContinuation { (cont: CheckedContinuation<Bool, Never>) in
            process.terminationHandler = { _ in cont.resume(returning: true) }
            do {
                try process.run()
            } catch {
                process.terminationHandler = nil
                cont.resume(returning: false)
            }
        }

        let exitCode: Int32? = launched ? process.terminationStatus : nil
        let finishedAt = Date()

        if onOutput != nil {
            outPipe.fileHandleForReading.readabilityHandler = nil
            errPipe.fileHandleForReading.readabilityHandler = nil
            // Drain anything buffered after the last readability callback.
            let tail = outPipe.fileHandleForReading.readDataToEndOfFile()
            let errTail = errPipe.fileHandleForReading.readDataToEndOfFile()
            if let s = String(data: tail, encoding: .utf8), !s.isEmpty { onOutput?(s) }
            if let s = String(data: errTail, encoding: .utf8), !s.isEmpty { onOutput?(s) }
        }

        await store.record(accountId: accountId, command: command, trigger: trigger,
                           startedAt: startedAt, finishedAt: finishedAt, exitCode: exitCode)
        return exitCode
    }
}
