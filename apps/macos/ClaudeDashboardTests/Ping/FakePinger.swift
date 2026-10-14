import Foundation
@testable import ClaudeDashboard

/// Records each ping's org id and answers with a settable result.
final class FakePinger: Pinging, @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [String] = []
    private var result: PingResult

    init(result: PingResult = PingResult(outcome: .ok, detail: nil)) {
        self.result = result
    }

    var calls: [String] {
        lock.lock(); defer { lock.unlock() }
        return recorded
    }

    func setResult(_ newResult: PingResult) {
        lock.lock(); result = newResult; lock.unlock()
    }

    func ping(orgId: String, sessionKey: String) async -> PingResult {
        lock.lock(); defer { lock.unlock() }
        recorded.append(orgId)
        return result
    }
}

/// Holds every ping until `release()`, so a test can look at the in-flight state.
final class GatedPinger: Pinging, @unchecked Sendable {
    private let lock = NSLock()
    private var waiting: [CheckedContinuation<Void, Never>] = []
    private var count = 0

    var startedCount: Int {
        lock.lock(); defer { lock.unlock() }
        return count
    }

    func ping(orgId: String, sessionKey: String) async -> PingResult {
        await withCheckedContinuation { continuation in
            lock.lock(); waiting.append(continuation); count += 1; lock.unlock()
        }
        return PingResult(outcome: .ok, detail: nil)
    }

    func release() {
        lock.lock(); let all = waiting; waiting = []; lock.unlock()
        all.forEach { $0.resume() }
    }
}
