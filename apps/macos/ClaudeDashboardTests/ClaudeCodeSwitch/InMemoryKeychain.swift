@testable import ClaudeDashboard
import Foundation

/// In-memory `KeychainStoring`, safe to call from several threads.
/// `failWrites` makes every write throw; `failWritesAfter = n` lets the next `n`
/// writes succeed and fails every later one (for rollback tests).
final class InMemoryKeychain: KeychainStoring {
    private let lock = NSLock()
    private var storage: [String: Data] = [:]
    private var _failWrites = false
    private var _failWritesAfter: Int?

    var items: [String: Data] {
        get { lock.lock(); defer { lock.unlock() }; return storage }
        set { lock.lock(); defer { lock.unlock() }; storage = newValue }
    }
    var failWrites: Bool {
        get { lock.lock(); defer { lock.unlock() }; return _failWrites }
        set { lock.lock(); defer { lock.unlock() }; _failWrites = newValue }
    }
    var failWritesAfter: Int? {
        get { lock.lock(); defer { lock.unlock() }; return _failWritesAfter }
        set { lock.lock(); defer { lock.unlock() }; _failWritesAfter = newValue }
    }

    func read(service: String, account: String) throws -> Data? {
        lock.lock(); defer { lock.unlock() }
        return storage["\(service)|\(account)"]
    }

    func write(_ data: Data, service: String, account: String) throws {
        lock.lock(); defer { lock.unlock() }
        if _failWrites { throw KeychainError.commandFailed(status: 1) }
        if let remaining = _failWritesAfter {
            if remaining <= 0 { throw KeychainError.commandFailed(status: 1) }
            _failWritesAfter = remaining - 1
        }
        storage["\(service)|\(account)"] = data
    }
}
