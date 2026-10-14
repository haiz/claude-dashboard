import XCTest
@testable import ClaudeDashboard

@MainActor
final class PingLogViewModelTests: XCTestCase {
    private var dbPath: String!
    private var suiteName: String!
    private var store: PingLogStore!
    private var accountStore: AccountStore!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "ping_log_vm_\(UUID().uuidString).db"
        store = PingLogStore(dbPath: dbPath)
        suiteName = StoreFixture.makeSuiteName()
        accountStore = AccountStore(defaults: UserDefaults(suiteName: suiteName)!)
    }

    override func tearDown() async throws {
        store = nil
        accountStore = nil
        StoreFixture.destroy(suite: suiteName)
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    private func record(_ accountId: UUID) async {
        await store.insert(accountId: accountId, trigger: .manual, startedAt: Date(), finishedAt: Date(),
                           result: PingResult(outcome: .ok, detail: nil))
    }

    func testLoadReturnsLoggedPings() async {
        await record(UUID())
        let vm = PingLogViewModel(store: store, accountStore: accountStore)

        await vm.load()

        XCTAssertEqual(vm.entries.count, 1)
    }

    func testClearEmptiesBothTheListAndTheStore() async {
        await record(UUID())
        let vm = PingLogViewModel(store: store, accountStore: accountStore)
        await vm.load()

        await vm.clear()
        let reloaded = await store.fetch(limit: 10)

        XCTAssertTrue(vm.entries.isEmpty)
        XCTAssertTrue(reloaded.isEmpty)
    }

    func testClearCallsOnClear() async {
        var called = false
        let vm = PingLogViewModel(store: store, accountStore: accountStore, onClear: { called = true })

        await vm.clear()

        XCTAssertTrue(called)
    }

    func testAccountNameForALiveAndARemovedAccount() async {
        let account = Account(id: UUID(), name: "Work", email: nil, chromeProfilePath: "Profile 1",
                              chromeProfileName: nil, orgId: "org-1", plan: .pro, lastSynced: nil,
                              status: .active, source: .browser)
        accountStore.addAccount(account)
        let vm = PingLogViewModel(store: store, accountStore: accountStore)

        XCTAssertEqual(vm.accountName(for: account.id), "Work")
        XCTAssertEqual(vm.accountName(for: UUID()), "Removed account")
    }
}
