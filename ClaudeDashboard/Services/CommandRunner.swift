// ClaudeDashboard/Services/CommandRunner.swift
import Foundation

/// The single place the app launches shell commands. Runs `command` via `/bin/zsh -c`
/// in the user's home dir, sourcing ~/.zshrc so shell functions/aliases/PATH resolve.
/// Enforces a timeout, supports cancellation, kills the whole process tree on
/// timeout/cancel (so caffeinate/claude grandchildren do not leak), captures a bounded
/// output tail, and records one row to CommandLogStore.
struct CommandRunner: Sendable {
    let store: CommandLogStore
    let registry: RunningProcessRegistry
    let timeout: TimeInterval

    init(store: CommandLogStore,
         registry: RunningProcessRegistry = RunningProcessRegistry(),
         timeout: TimeInterval = 60) {
        self.store = store
        self.registry = registry
        self.timeout = timeout
    }

    @discardableResult
    func run(command: String,
             accountId: UUID?,
             trigger: CommandTrigger,
             onOutput: (@Sendable (String) -> Void)? = nil) async -> CommandResult {
        let startedAt = Date()
        let process = Process()
        process.launchPath = "/bin/zsh"
        // See design: source ~/.zshrc (stderr suppressed) so functions like `ccbf` resolve.
        process.arguments = ["-c", "source ~/.zshrc 2>/dev/null\n\(command)"]
        process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())

        let outPipe = Pipe()
        let errPipe = Pipe()
        process.standardOutput = outPipe
        process.standardError = errPipe

        let tail = OutputTail(maxBytes: 4096)
        let sink: @Sendable (Data) -> Void = { data in
            guard !data.isEmpty, let s = String(data: data, encoding: .utf8) else { return }
            tail.append(s)
            onOutput?(s)
        }
        outPipe.fileHandleForReading.readabilityHandler = { sink($0.availableData) }
        errPipe.fileHandleForReading.readabilityHandler = { sink($0.availableData) }

        // Launch. On failure, record and return immediately.
        do {
            try process.run()
        } catch {
            outPipe.fileHandleForReading.readabilityHandler = nil
            errPipe.fileHandleForReading.readabilityHandler = nil
            let msg = "launch failed: \(error.localizedDescription)"
            await store.record(accountId: accountId, command: command, trigger: trigger,
                               startedAt: startedAt, finishedAt: Date(),
                               status: .launchFailed, exitCode: nil, output: msg)
            return CommandResult(status: .launchFailed, exitCode: nil, outputTail: msg)
        }

        let pid = process.processIdentifier
        registry.add(process)
        let state = RunState()

        // Timeout watchdog: SIGTERM the tree, grace, then SIGKILL. Marks status first so
        // the resume path (terminationHandler) reports .timedOut.
        let timeoutTask = Task { [timeout] in
            try? await Task.sleep(nanoseconds: UInt64(timeout * 1_000_000_000))
            guard !state.isFinished else { return }
            state.markTimedOut()
            if process.isRunning { ProcessTree.killTree(pid, signal: SIGTERM) }
            try? await Task.sleep(nanoseconds: 2_000_000_000)
            if process.isRunning { ProcessTree.killTree(pid, signal: SIGKILL) }
        }

        await withTaskCancellationHandler {
            await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
                process.terminationHandler = { _ in
                    if state.finish() { cont.resume() }
                }
                // Cover the race where the process exited before the handler was set.
                if !process.isRunning, state.finish() { cont.resume() }
            }
        } onCancel: {
            state.markCancelled()
            if process.isRunning { ProcessTree.killTree(pid, signal: SIGKILL) }
        }

        timeoutTask.cancel()
        registry.remove(pid: pid)

        // Drain anything buffered after the last readability callback.
        outPipe.fileHandleForReading.readabilityHandler = nil
        errPipe.fileHandleForReading.readabilityHandler = nil
        let outRest = outPipe.fileHandleForReading.readDataToEndOfFile()
        let errRest = errPipe.fileHandleForReading.readDataToEndOfFile()
        if let s = String(data: outRest, encoding: .utf8), !s.isEmpty { tail.append(s); onOutput?(s) }
        if let s = String(data: errRest, encoding: .utf8), !s.isEmpty { tail.append(s); onOutput?(s) }

        let finishedAt = Date()
        let status = state.status
        let exitCode: Int32? = (status == .exited) ? process.terminationStatus : nil
        let outputTail = tail.string
        await store.record(accountId: accountId, command: command, trigger: trigger,
                           startedAt: startedAt, finishedAt: finishedAt,
                           status: status, exitCode: exitCode, output: outputTail)
        return CommandResult(status: status, exitCode: exitCode, outputTail: outputTail)
    }
}

/// One-shot completion + status latch guarding the run's continuation.
private final class RunState: @unchecked Sendable {
    private let lock = NSLock()
    private var _finished = false
    private var _status: CommandStatus = .exited

    var isFinished: Bool { lock.lock(); defer { lock.unlock() }; return _finished }
    var status: CommandStatus { lock.lock(); defer { lock.unlock() }; return _status }

    /// Returns true exactly once, on the first caller; marks finished.
    func finish() -> Bool {
        lock.lock(); defer { lock.unlock() }
        if _finished { return false }
        _finished = true
        return true
    }

    func markTimedOut() { lock.lock(); if !_finished { _status = .timedOut }; lock.unlock() }
    func markCancelled() { lock.lock(); if !_finished { _status = .cancelled }; lock.unlock() }
}

/// Thread-safe bounded tail of streamed output.
private final class OutputTail: @unchecked Sendable {
    private let lock = NSLock()
    private var buf = ""
    private let maxBytes: Int
    init(maxBytes: Int) { self.maxBytes = maxBytes }
    func append(_ s: String) {
        lock.lock()
        buf += s
        if buf.utf8.count > maxBytes { buf = String(buf.suffix(maxBytes)) }
        lock.unlock()
    }
    var string: String { lock.lock(); defer { lock.unlock() }; return buf }
}
