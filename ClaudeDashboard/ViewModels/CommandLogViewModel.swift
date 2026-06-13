// ClaudeDashboard/ViewModels/CommandLogViewModel.swift
import Foundation
import Combine

@MainActor
final class CommandLogViewModel: ObservableObject {
    @Published var entries: [CommandLogEntry] = []

    private let store: CommandLogStore
    private let accountStore: AccountStore
    private let limit: Int

    init(store: CommandLogStore, accountStore: AccountStore, limit: Int = 500) {
        self.store = store
        self.accountStore = accountStore
        self.limit = limit
    }

    func load() async {
        entries = await store.recent(limit: limit)
    }

    func clear() async {
        await store.clear()
        entries = []
    }

    /// Human-readable account name for a logged row. "—" when no account was
    /// associated, "Unknown" when the account no longer exists.
    func accountName(for id: UUID?) -> String {
        guard let id else { return "—" }
        return accountStore.accounts.first(where: { $0.id == id })?.name ?? "Unknown"
    }
}
