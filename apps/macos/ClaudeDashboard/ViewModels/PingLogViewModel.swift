import Foundation
import Combine

/// State of the Ping Log pane.
///
///     let vm = PingLogViewModel(store: dashboard.pingController.logStore,
///                               accountStore: dashboard.accountStore)
///     await vm.load()
@MainActor
final class PingLogViewModel: ObservableObject {
    @Published var entries: [PingLogEntry] = []

    private let store: PingLogStore
    private let accountStore: AccountStore
    private let limit: Int

    init(store: PingLogStore, accountStore: AccountStore, limit: Int = 500) {
        self.store = store
        self.accountStore = accountStore
        self.limit = limit
    }

    func load() async {
        entries = await store.fetch(limit: limit)
    }

    func clear() async {
        await store.clear()
        entries = []
    }

    /// "Removed account" once the account is gone; its rows stay until they age out.
    func accountName(for id: UUID) -> String {
        accountStore.accounts.first(where: { $0.id == id })?.name ?? "Removed account"
    }
}
