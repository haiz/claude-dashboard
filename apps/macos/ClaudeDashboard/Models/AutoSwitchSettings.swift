import Foundation

/// Auto switch settings and the priority tick, persisted as soon as they change.
/// The app passes `AppDefaults.shared`; tests pass a suite of their own.
///
///     let settings = AutoSwitchSettings(defaults: AppDefaults.shared)
///     settings.isEnabled = true
///     settings.thresholds   // AutoSwitchThresholds(fiveHour: 95, sevenDay: 99)
///     settings.setPriority(AutoSwitchPriority(accountId: id, baseline: resetsAt))
///     settings.setExcluded(id, true)   // auto switch never moves Claude Code to `id`
@MainActor
final class AutoSwitchSettings: ObservableObject {
    static let enabledKey = "autoSwitchEnabled"
    static let fiveHourKey = "autoSwitchFiveHourThreshold"
    static let sevenDayKey = "autoSwitchSevenDayThreshold"
    static let showWidgetToggleKey = "autoSwitchShowWidgetToggle"
    static let priorityIdKey = "autoSwitchPriorityAccountId"
    static let priorityBaselineKey = "autoSwitchPriorityBaseline"
    static let excludedKey = "autoSwitchExcludedAccountIds"
    static let fiveHourRange = 50...99
    static let sevenDayRange = 50...100
    static let defaultFiveHour = 95
    static let defaultSevenDay = 99

    private let defaults: UserDefaults

    @Published var isEnabled: Bool {
        didSet { defaults.set(isEnabled, forKey: Self.enabledKey) }
    }
    @Published var fiveHourThreshold: Int {
        didSet { defaults.set(fiveHourThreshold, forKey: Self.fiveHourKey) }
    }
    @Published var sevenDayThreshold: Int {
        didSet { defaults.set(sevenDayThreshold, forKey: Self.sevenDayKey) }
    }
    /// Lets the popover footer show the toggle when the active account nears a limit.
    @Published var showWidgetToggle: Bool {
        didSet { defaults.set(showWidgetToggle, forKey: Self.showWidgetToggleKey) }
    }
    @Published private(set) var priority: AutoSwitchPriority?
    /// Accounts auto switch never moves Claude Code to (a manual Switch still can).
    @Published private(set) var excluded: Set<UUID>

    var thresholds: AutoSwitchThresholds {
        AutoSwitchThresholds(fiveHour: Double(fiveHourThreshold), sevenDay: Double(sevenDayThreshold))
    }

    init(defaults: UserDefaults) {
        self.defaults = defaults
        isEnabled = defaults.object(forKey: Self.enabledKey) as? Bool ?? true
        fiveHourThreshold = Self.stored(defaults, Self.fiveHourKey, in: Self.fiveHourRange, default: Self.defaultFiveHour)
        sevenDayThreshold = Self.stored(defaults, Self.sevenDayKey, in: Self.sevenDayRange, default: Self.defaultSevenDay)
        showWidgetToggle = defaults.object(forKey: Self.showWidgetToggleKey) as? Bool ?? true
        priority = defaults.string(forKey: Self.priorityIdKey).flatMap(UUID.init(uuidString:)).map { id in
            AutoSwitchPriority(
                accountId: id,
                baseline: (defaults.object(forKey: Self.priorityBaselineKey) as? Double)
                    .map(Date.init(timeIntervalSince1970:)))
        }
        excluded = Set((defaults.stringArray(forKey: Self.excludedKey) ?? []).compactMap(UUID.init(uuidString:)))
    }

    /// Restores every setting to its default; the home account (`priority`) and the
    /// excluded accounts are per-account choices and are kept.
    func resetToDefaults() {
        isEnabled = true
        fiveHourThreshold = Self.defaultFiveHour
        sevenDayThreshold = Self.defaultSevenDay
        showWidgetToggle = true
    }

    func setPriority(_ priority: AutoSwitchPriority?) {
        self.priority = priority
        guard let priority else {
            defaults.removeObject(forKey: Self.priorityIdKey)
            defaults.removeObject(forKey: Self.priorityBaselineKey)
            return
        }
        defaults.set(priority.accountId.uuidString, forKey: Self.priorityIdKey)
        if let baseline = priority.baseline {
            defaults.set(baseline.timeIntervalSince1970, forKey: Self.priorityBaselineKey)
        } else {
            defaults.removeObject(forKey: Self.priorityBaselineKey)
        }
    }

    /// Excluding the home account unticks it: auto switch can't move there any more.
    func setExcluded(_ id: UUID, _ on: Bool) {
        var ids = excluded
        if on { ids.insert(id) } else { ids.remove(id) }
        store(excluded: ids)
        if on, priority?.accountId == id { setPriority(nil) }
    }

    /// Drops the ids of accounts that no longer exist.
    func pruneExcluded(keeping ids: Set<UUID>) {
        let kept = excluded.intersection(ids)
        if kept != excluded { store(excluded: kept) }
    }

    private func store(excluded ids: Set<UUID>) {
        excluded = ids
        if ids.isEmpty {
            defaults.removeObject(forKey: Self.excludedKey)
        } else {
            defaults.set(ids.map(\.uuidString).sorted(), forKey: Self.excludedKey)
        }
    }

    private static func stored(_ defaults: UserDefaults, _ key: String,
                               in range: ClosedRange<Int>, default value: Int) -> Int {
        guard let stored = defaults.object(forKey: key) as? Int, range.contains(stored) else { return value }
        return stored
    }
}
