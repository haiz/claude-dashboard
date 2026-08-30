// ClaudeDashboard/Services/ProcessTree.swift
import Foundation
import Darwin

/// Process-tree utilities. Foundation `Process.terminate()` signals only the direct
/// child, so a `/bin/zsh -c` that spawns `caffeinate` -> `claude` would leak those
/// grandchildren. These helpers walk the tree via libproc and signal every descendant.
enum ProcessTree {
    /// Immediate child pids of `pid` (empty on error or no children).
    static func childPids(of pid: Int32) -> [Int32] {
        let needed = proc_listchildpids(pid, nil, 0)
        guard needed > 0 else { return [] }
        let capacity = Int(needed) + 16   // headroom for children spawned meanwhile
        var buffer = [pid_t](repeating: 0, count: capacity)
        // NOTE: proc_listchildpids returns a pid *count* here, not a byte length
        // (confirmed empirically: filling a buffer with N real children yields a
        // raw return value of exactly N). Do not divide by MemoryLayout<pid_t>.size
        // again -- that silently truncates any real result below 4 children to 0.
        let count = proc_listchildpids(pid, &buffer, Int32(capacity * MemoryLayout<pid_t>.size))
        guard count > 0 else { return [] }
        return buffer.prefix(Int(count)).map { Int32($0) }.filter { $0 > 0 }
    }

    /// All transitive descendants of `pid`, depth-first.
    static func descendants(of pid: Int32) -> [Int32] {
        var out: [Int32] = []
        var stack = childPids(of: pid)
        while let next = stack.popLast() {
            out.append(next)
            stack.append(contentsOf: childPids(of: next))
        }
        return out
    }

    /// Signal every descendant then the root. `kill` on an already-dead pid is a
    /// harmless no-op (ESRCH), so races during teardown are safe.
    static func killTree(_ pid: Int32, signal: Int32) {
        for child in descendants(of: pid) { kill(child, signal) }
        kill(pid, signal)
    }
}
