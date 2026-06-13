// ClaudeDashboardTests/CommandLogViewModelTests.swift
import XCTest
@testable import ClaudeDashboard

@MainActor
final class CommandLogViewModelTests: XCTestCase {
    var store: CommandLogStore!
    var dbPath: String!
    var accountStore: AccountStore!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "test_clvm_\(UUID().uuidString).db"
        store = await CommandLogStore(dbPath: dbPath)
        let suite = "test_clvm_\(UUID().uuidString)"
        accountStore = AccountStore(defaults: UserDefaults(suiteName: suite)!)
    }

    override func tearDown() async throws {
        store = nil
        accountStore = nil
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    func testLoadReturnsRecordedEntries() async {
        await store.record(accountId: nil, command: "echo a", trigger: .manual,
                           startedAt: Date(), finishedAt: Date(), exitCode: 0)
        let vm = CommandLogViewModel(store: store, accountStore: accountStore)
        await vm.load()
        XCTAssertEqual(vm.entries.count, 1)
        XCTAssertEqual(vm.entries[0].command, "echo a")
    }

    func testClearEmptiesEntries() async {
        await store.record(accountId: nil, command: "x", trigger: .manual,
                           startedAt: Date(), finishedAt: Date(), exitCode: 0)
        let vm = CommandLogViewModel(store: store, accountStore: accountStore)
        await vm.load()
        await vm.clear()
        XCTAssertTrue(vm.entries.isEmpty)
        // Reload from the store to prove it was flushed, not just the cache.
        await vm.load()
        XCTAssertTrue(vm.entries.isEmpty)
    }

    func testAccountNameFallbackForUnknown() {
        let vm = CommandLogViewModel(store: store, accountStore: accountStore)
        XCTAssertEqual(vm.accountName(for: nil), "—")
        XCTAssertEqual(vm.accountName(for: UUID()), "Unknown")
    }
}
