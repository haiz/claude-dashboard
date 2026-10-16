import Foundation

/// Critical-account settings, persisted as soon as they change. The app passes
/// `AppDefaults.shared`; tests pass a suite of their own.
///
///     let settings = CriticalSettings(defaults: AppDefaults.shared)
///     settings.pace = 50                       // % of 5h per window, quiet hours excluded
///     settings.parameters(x: 16.3)             // feeds CriticalPlanner.assess
@MainActor
final class CriticalSettings: ObservableObject {
    static let paceKey = "criticalPace"
    static let highPaceKey = "criticalHighPace"
    static let quietStartKey = "criticalQuietStart"
    static let quietEndKey = "criticalQuietEnd"
    static let timeZoneKey = "criticalTimeZone"
    static let paceRange = 10...100
    static let paceStep = 5
    static let hourRange = 0...23
    static let defaultTimeZone = "Asia/Ho_Chi_Minh"

    private let defaults: UserDefaults

    /// Raising it above `highPace` raises `highPace` with it.
    @Published var pace: Int {
        didSet {
            defaults.set(pace, forKey: Self.paceKey)
            if highPace < pace { highPace = pace }
        }
    }
    /// Never below `pace`.
    @Published var highPace: Int {
        didSet {
            if highPace < pace { highPace = pace }
            defaults.set(highPace, forKey: Self.highPaceKey)
        }
    }
    @Published var quietStart: Int {
        didSet { defaults.set(quietStart, forKey: Self.quietStartKey) }
    }
    @Published var quietEnd: Int {
        didSet { defaults.set(quietEnd, forKey: Self.quietEndKey) }
    }
    @Published var timeZoneId: String {
        didSet { defaults.set(timeZoneId, forKey: Self.timeZoneKey) }
    }

    init(defaults: UserDefaults) {
        self.defaults = defaults
        let pace = Self.stored(defaults, Self.paceKey, in: Self.paceRange, default: 50)
        self.pace = pace
        highPace = max(Self.stored(defaults, Self.highPaceKey, in: Self.paceRange, default: 80), pace)
        quietStart = Self.stored(defaults, Self.quietStartKey, in: Self.hourRange, default: 0)
        quietEnd = Self.stored(defaults, Self.quietEndKey, in: Self.hourRange, default: 7)
        let zone = defaults.string(forKey: Self.timeZoneKey)
        timeZoneId = zone.flatMap(TimeZone.init(identifier:)) != nil ? zone! : Self.defaultTimeZone
    }

    /// nil when start == end (no quiet hours).
    var quietHours: QuietHours? {
        guard quietStart != quietEnd else { return nil }
        let zone = TimeZone(identifier: timeZoneId) ?? TimeZone(identifier: Self.defaultTimeZone)!
        return QuietHours(startHour: quietStart, endHour: quietEnd, timeZone: zone)
    }

    func parameters(x: Double) -> CriticalParameters {
        CriticalParameters(x: x, pace: Double(pace), highPace: Double(highPace), quiet: quietHours)
    }

    private static func stored(_ defaults: UserDefaults, _ key: String,
                               in range: ClosedRange<Int>, default value: Int) -> Int {
        guard let stored = defaults.object(forKey: key) as? Int, range.contains(stored) else { return value }
        return stored
    }
}
