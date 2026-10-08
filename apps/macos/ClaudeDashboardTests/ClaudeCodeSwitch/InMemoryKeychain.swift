@testable import ClaudeDashboard
import Foundation

/// In-memory `KeychainStoring`. `failWrites` makes every write throw, for rollback tests.
final class InMemoryKeychain: KeychainStoring {
    var items: [String: Data] = [:]
    var failWrites = false

    func read(service: String, account: String) throws -> Data? {
        items["\(service)|\(account)"]
    }

    func write(_ data: Data, service: String, account: String) throws {
        if failWrites { throw KeychainError.commandFailed(status: 1) }
        items["\(service)|\(account)"] = data
    }
}
