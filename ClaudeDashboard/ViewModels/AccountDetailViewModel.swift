import Foundation
import SwiftUI

@MainActor
final class AccountDetailViewModel: ObservableObject {
    let accountId: UUID
    let accountName: String
    let accountPlan: AccountPlan
    private let logStore: UsageLogStore

    @Published var selectedWindow: UsageWindow = .fiveHour
    @Published var logs: [UsageLogEntry] = []
    @Published var resetCycles: [ResetCycle] = []
    @Published var selectedCycle: ResetCycle?

    init(accountId: UUID, accountName: String, accountPlan: AccountPlan, logStore: UsageLogStore) {
        self.accountId = accountId
        self.accountName = accountName
        self.accountPlan = accountPlan
        self.logStore = logStore
    }

    func loadData() async {
        let cycles = await logStore.resetCycles(accountId: accountId, window: selectedWindow)
        resetCycles = cycles

        if let cycle = selectedCycle {
            let cycleLogs = await logStore.logs(
                accountId: accountId, window: selectedWindow,
                from: cycle.firstRecordedAt.addingTimeInterval(-1),
                to: cycle.resetsAt
            )
            logs = cycleLogs
        } else {
            let allLogs = await logStore.logs(
                accountId: accountId, window: selectedWindow,
                from: nil, to: nil
            )
            logs = allLogs
        }
    }

    func selectWindow(_ window: UsageWindow) {
        selectedWindow = window
        selectedCycle = nil
        Task { await loadData() }
    }

    func selectCycle(_ cycle: ResetCycle?) {
        selectedCycle = cycle
        Task { await loadData() }
    }
}
