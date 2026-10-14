import Foundation

/// Ping glue: building targets, the account page's button, the Auto Ping toggle,
/// and firing auto pings after a refresh. Rules live in `PingController`.
extension DashboardViewModel {

    func pingTarget(for account: Account) -> PingTarget? {
        guard let orgId = account.orgId,
              let sessionKey = accountStore.loadSessionKey(for: account.id) else { return nil }
        return PingTarget(accountId: account.id, orgId: orgId, sessionKey: sessionKey)
    }

    /// The account page's Ping button is enabled for an active account with a key,
    /// while no ping for it is running.
    func canPing(_ state: AccountUsageState) -> Bool {
        state.account.status == .active
            && !pingController.inFlight.contains(state.id)
            && pingTarget(for: state.account) != nil
    }

    /// The Ping button: ping, then refresh that account so its reset time shows.
    func pingNow(_ accountId: UUID) async {
        guard let account = accountStates.first(where: { $0.id == accountId })?.account,
              let target = pingTarget(for: account) else { return }
        if await pingController.ping(target, trigger: .manual) != nil {
            await refreshAll(only: [accountId])
        }
    }

    /// Turning Auto Ping on refreshes now, so idle accounts are pinged without
    /// waiting for the next interval.
    func setAutoPingEnabled(_ on: Bool) {
        pingController.isAutoPingEnabled = on
        if on { Task { await refreshAll() } }
    }

    /// Pings `targets` in parallel in the background, then refreshes them.
    func runAutoPings(_ targets: [PingTarget]) {
        guard !targets.isEmpty else { return }
        let controller = pingController
        autoPingTask = Task {
            await withTaskGroup(of: Void.self) { group in
                for target in targets {
                    group.addTask {
                    if await controller.ping(target, trigger: .auto) == nil {
                        await controller.releaseClaim(target.accountId)
                    }
                }
                }
            }
            await refreshAll(only: Set(targets.map(\.accountId)))
        }
    }
}
