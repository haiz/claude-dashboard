import Foundation

/// Resolves the running app's marketing version from `Info.plist`.
///
/// Source of truth is the repo-root `VERSION` file; `scripts/sync-version.sh`
/// propagates it to `CFBundleShortVersionString`, which this helper reads at
/// runtime.
enum AppVersion {
    /// Semver string (e.g. `"1.1.2"`), or `"—"` if the key is missing.
    static var string: String {
        (Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String) ?? "—"
    }

    /// Day this version was released, from `CDReleaseDate` (`yyyy-MM-dd`,
    /// written by `scripts/release.sh`), or nil if the key is missing or malformed.
    static var releaseDate: Date? {
        (Bundle.main.object(forInfoDictionaryKey: "CDReleaseDate") as? String).flatMap(parseReleaseDate)
    }

    static func parseReleaseDate(_ raw: String) -> Date? {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone.current
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.date(from: raw)
    }
}
