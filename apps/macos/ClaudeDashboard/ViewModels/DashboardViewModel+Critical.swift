import Foundation

/// Critical glue: one assessment per account state from the current X and settings.
/// Rules live in `CriticalPlanner`; this file only feeds it.
extension DashboardViewModel {

    /// `.none` for a stale fetch or an account that is not active.
    func criticalAssessment(for state: AccountUsageState, now: Date = Date()) -> CriticalAssessment {
        guard state.error == nil, state.account.status == .active else { return .none }
        return CriticalPlanner.assess(state.usage, params: criticalParameters, now: now)
    }

    func criticalBadge(for state: AccountUsageState) -> CriticalBadge? {
        CriticalPlanner.badge(for: criticalAssessment(for: state), params: criticalParameters)
    }

    private var criticalParameters: CriticalParameters {
        criticalSettings.parameters(x: ratioController.current.x)
    }
}
