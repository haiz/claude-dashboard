import Foundation
import Darwin

/// Spawns the real `claude-dashboard-helper` binary.
///
/// Two details that bite if skipped: `Process.environment` *replaces* the
/// environment rather than extending it, so the parent's environment is copied
/// first; and `AddKeyCommand` blocks on `readDataToEndOfFile()`, so stdin must
/// be closed, not merely written.
enum HelperProcess {

    struct Result {
        let stdout: Data
        let stderr: String
        let exitCode: Int32

        var stdoutText: String { String(decoding: stdout, as: UTF8.self) }
    }

    static var binaryURL: URL {
        Bundle(for: LoopbackServer.self).bundleURL
            .deletingLastPathComponent()
            .appendingPathComponent("claude-dashboard-helper")
    }

    static func run(_ args: [String], stdin: String? = nil, env extra: [String: String] = [:]) -> Result {
        let process = Process()
        process.executableURL = binaryURL
        process.arguments = args

        var environment = ProcessInfo.processInfo.environment
        // URLSession writes diagnostics to stderr when this is set, which would
        // break every byte-exact stderr assertion. A developer shell may have it.
        environment.removeValue(forKey: "CFNETWORK_DIAGNOSTICS")
        // xcodebuild/Xcode's test runner sets OS_ACTIVITY_DT_MODE=YES on itself so
        // os_log activity shows in the Xcode console; HelperProcess inherits it via
        // ProcessInfo.processInfo.environment, and the child mirrors os_log traffic
        // (e.g. Network.framework's nw_path_necp_check_for_updates diagnostics on a
        // dropped connection) onto its OWN stderr as a result — polluting the
        // byte-exact stderr contract on transport-failure paths. A real user running
        // the binary from a shell has no such variable and sees clean stderr, so the
        // test harness must not inherit it either.
        environment.removeValue(forKey: "OS_ACTIVITY_DT_MODE")
        for (key, value) in extra { environment[key] = value }
        process.environment = environment

        let outPipe = Pipe(), errPipe = Pipe(), inPipe = Pipe()
        process.standardOutput = outPipe
        process.standardError = errPipe
        process.standardInput = inPipe

        // Drain both pipes concurrently: a child that fills one pipe's buffer
        // while the parent reads the other would deadlock. Each drained into its
        // own reference box, so no `var` is captured and mutated across queues.
        final class Box: @unchecked Sendable { var data = Data() }
        let outBox = Box(), errBox = Box()
        let outHandle = outPipe.fileHandleForReading
        let errHandle = errPipe.fileHandleForReading
        let group = DispatchGroup()

        do {
            try process.run()
        } catch {
            fatalError("could not run \(binaryURL.path): \(error)")
        }

        group.enter()
        DispatchQueue.global().async { outBox.data = outHandle.readDataToEndOfFile(); group.leave() }
        group.enter()
        DispatchQueue.global().async { errBox.data = errHandle.readDataToEndOfFile(); group.leave() }

        // The child may already have exited without reading stdin -- which is
        // exactly what `add-key`'s argument-rejection paths do. Writing to that
        // pipe gets EPIPE, and EPIPE raises SIGPIPE, whose disposition in this
        // process is SIG_DFL (measured, not assumed), so it would kill the
        // whole test bundle rather than fail one test. Both `write(_:)` and
        // `write(contentsOf:)` were measured doing exactly that.
        // F_SETNOSIGPIPE turns it into a plain error, and `try?` discards it:
        // a helper that did not want its stdin is not a test failure.
        let stdinHandle = inPipe.fileHandleForWriting
        _ = fcntl(stdinHandle.fileDescriptor, F_SETNOSIGPIPE, 1)
        if let stdin { try? stdinHandle.write(contentsOf: Data(stdin.utf8)) }
        try? stdinHandle.close()

        process.waitUntilExit()
        group.wait()

        return Result(
            stdout: outBox.data,
            stderr: String(decoding: errBox.data, as: UTF8.self),
            exitCode: process.terminationStatus
        )
    }
}
