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
}
