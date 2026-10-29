import Foundation

/// Fast refresh: an account burning at cheetah or rocket on its 5h window is fetched every 60 s.
/// The rule is `BurnRateResult.needsFastRefresh` (contract/README.md, "Fast refresh").
extension DashboardViewModel {

    /// Accounts the 60 s poll refreshes: 5h level cheetah or rocket, active, no fetch error,
    /// Auto Refresh on. Independent of `showBurnAnimals`.
    func burnFastIDs() -> Set<UUID> {
        Set(accountStates.filter { state in
            BurnRateResult.needsFastRefresh(fiveHourLevel: state.burnRates?.fiveHour?.level,
                                            isActive: state.account.status == .active,
                                            hasError: state.error != nil,
                                            autoRefreshEnabled: autoRefreshEnabled)
        }.map(\.id))
    }

    /// Stops the poll when no account needs it, starts it when one does, and re-arms a running
    /// poll after a full refresh so its first fetch is 60 s after that refresh.
    /// Each poll runs `refreshAll(only:)`, which calls back here.
    func updateBurnPoll(afterFullRefresh: Bool) {
        guard !burnFastIDs().isEmpty else {
            stopBurnPoll()
            return
        }
        if burnPollTask != nil {
            guard afterFullRefresh else { return }
            burnPollTask?.cancel()
        }
        burnPollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 60_000_000_000)
                guard !Task.isCancelled, let self else { return }
                let ids = self.burnFastIDs()
                guard !ids.isEmpty else {
                    self.burnPollTask = nil
                    return
                }
                await self.refreshAll(only: ids)
            }
        }
    }

    func stopBurnPoll() {
        burnPollTask?.cancel()
        burnPollTask = nil
    }
}
