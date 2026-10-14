import Foundation

/// Auto switch glue: the snapshot, the footer and tick state, the 60 s poll.
/// Rules live in `AutoSwitchPlanner`; this file only feeds it.
extension DashboardViewModel {

    var autoSwitchAccounts: [AutoSwitchAccount] {
        accountStates.map { state in
            AutoSwitchAccount(id: state.id,
                              displayName: state.account.email ?? state.account.name,
                              usage: state.usage,
                              availability: switchAvailability[state.id],
                              isUsable: state.account.status == .active,
                              isActive: isActiveClaudeCodeAccount(state),
                              isFresh: state.error == nil)
        }
    }

    /// The popover footer shows the toggle while auto switch is on, and near a limit when
    /// the user lets it (`showWidgetToggle`).
    var showsAutoSwitchToggle: Bool {
        autoSwitchSettings.isEnabled || (autoSwitchSettings.showWidgetToggle && isAutoSwitchNearLimit)
    }

    /// The active account is in the toggle zone. Outside it the footer dims an on toggle.
    var isAutoSwitchNearLimit: Bool {
        guard let usage = accountStates.first(where: isActiveClaudeCodeAccount)?.usage else { return false }
        return AutoSwitchPlanner.isInToggleZone(usage, thresholds: autoSwitchSettings.thresholds)
    }

    /// Turning it on near a limit acts now, not at the next refresh.
    func setAutoSwitchEnabled(_ on: Bool) {
        autoSwitchSettings.isEnabled = on
        Task { await runAutoSwitch() }
    }

    func isPriorityTarget(_ id: UUID) -> Bool {
        autoSwitchSettings.priority?.accountId == id
    }

    /// A tick needs an account auto switch could move to, with loaded usage for the baseline.
    func canTickPriority(_ state: AccountUsageState) -> Bool {
        switchAvailability[state.id] == .ready && !isActiveClaudeCodeAccount(state) && state.usage != nil
    }

    /// Ticks `id` (moving the tick off any other account) or unticks it.
    func togglePriority(for id: UUID) {
        if isPriorityTarget(id) {
            autoSwitchSettings.setPriority(nil)
            return
        }
        let baseline = accountStates.first { $0.id == id }?.usage?.fiveHour.resetsAt
        autoSwitchSettings.setPriority(AutoSwitchPriority(accountId: id, baseline: baseline))
        Task { await runAutoSwitch() }
    }

    func runAutoSwitch() async {
        if !isSwitchingClaudeCode {
            await autoSwitch.evaluate(autoSwitchAccounts) { id in try await self.performAutoSwitch(to: id) }
        }
        updateAutoSwitchPoll()
    }

    /// Starts the 60 s poll of the active account while `shouldFastPoll`, stops it otherwise.
    /// Each poll runs `refreshAll(only:)`, which evaluates and calls back here.
    func updateAutoSwitchPoll() {
        guard autoSwitch.shouldFastPoll(autoSwitchAccounts) else {
            autoSwitchPollTask?.cancel()
            autoSwitchPollTask = nil
            return
        }
        guard autoSwitchPollTask == nil else { return }
        autoSwitchPollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 60_000_000_000)
                guard !Task.isCancelled, let self else { return }
                guard let activeId = self.accountStates.first(where: self.isActiveClaudeCodeAccount)?.id else {
                    self.autoSwitchPollTask = nil
                    return
                }
                await self.refreshAll(only: [activeId])
            }
        }
    }
}
