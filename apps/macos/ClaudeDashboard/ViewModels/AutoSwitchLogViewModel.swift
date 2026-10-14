import Foundation

/// State of the Switch Log pane.
///
///     let vm = AutoSwitchLogViewModel(store: dashboard.autoSwitch.logStore)
///     await vm.load()
@MainActor
final class AutoSwitchLogViewModel: ObservableObject {
    @Published var entries: [AutoSwitchLogEntry] = []

    private let store: AutoSwitchLogStore
    private let limit: Int

    init(store: AutoSwitchLogStore, limit: Int = 500) {
        self.store = store
        self.limit = limit
    }

    func load() async {
        entries = await store.fetch(limit: limit)
    }

    func clear() async {
        await store.clear()
        entries = []
    }
}
