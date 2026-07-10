// ClaudeDashboard/Services/RunningProcessRegistry.swift
import Foundation

/// Tracks pids of in-flight background command processes so the app can reap their
/// whole trees on quit. Shared by value-type CommandRunner copies (reference semantics).
final class RunningProcessRegistry: @unchecked Sendable {
    private let lock = NSLock()
    private var pids: Set<Int32> = []

    func add(_ process: Process) {
        lock.lock(); pids.insert(process.processIdentifier); lock.unlock()
    }

    func remove(pid: Int32) {
        lock.lock(); pids.remove(pid); lock.unlock()
    }

    var count: Int {
        lock.lock(); defer { lock.unlock() }; return pids.count
    }

    /// SIGKILL every tracked process tree. Called on app termination.
    func terminateAll() {
        lock.lock(); let snapshot = pids; lock.unlock()
        for pid in snapshot { ProcessTree.killTree(pid, signal: SIGKILL) }
    }
}
