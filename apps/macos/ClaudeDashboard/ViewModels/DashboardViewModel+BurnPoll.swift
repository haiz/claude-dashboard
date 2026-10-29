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
    ///
    /// A poll fetch in flight is never cancelled: cancelling it would write "cancelled" into the
    /// cards it is fetching. A re-arm during the fetch is a no-op, since the loop sleeps 60 s
    /// once the fetch returns.
    func updateBurnPoll(afterFullRefresh: Bool) {
        guard !burnFastIDs().isEmpty else {
            stopBurnPoll()
            return
        }
        if burnPollTask != nil {
            guard afterFullRefresh, !burnPollFetching else { return }
            burnPollTask?.cancel()
        }
        let token = UUID()
        burnPollToken = token
        burnPollTask = Task { [weak self] in
            while true {
                try? await Task.sleep(nanoseconds: 60_000_000_000)
                guard !Task.isCancelled, let self, self.burnPollToken == token else { return }
                let ids = self.burnFastIDs()
                guard !ids.isEmpty else {
                    self.stopBurnPoll()
                    return
                }
                self.burnPollFetching = true
                await self.refreshAll(only: ids)
                self.burnPollFetching = false
                // Stopped or replaced while fetching.
                guard self.burnPollToken == token else { return }
            }
        }
    }

    /// Ends the poll. A sleeping poll is cancelled; one mid-fetch finishes that fetch and then
    /// exits, because its token no longer matches.
    func stopBurnPoll() {
        if !burnPollFetching { burnPollTask?.cancel() }
        burnPollTask = nil
        burnPollToken = nil
    }
}
