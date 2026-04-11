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
    @Published var visibleRange: ClosedRange<Date> = {
        let now = Date()
        return now.addingTimeInterval(-86400)...now
    }()

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
            // Refresh range to current time so we always include the latest data
            let duration = visibleRange.upperBound.timeIntervalSince(visibleRange.lowerBound)
            let now = Date()
            visibleRange = now.addingTimeInterval(-duration)...now

            let allLogs = await logStore.logs(
                accountId: accountId, window: selectedWindow,
                from: visibleRange.lowerBound, to: visibleRange.upperBound
            )
            logs = allLogs
        }
    }

    func updateRange(_ range: ClosedRange<Date>) {
        visibleRange = range
        Task { await loadData() }
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
