//! Non-critical app settings (`settings.json`, next to `accounts.json`).
//! Unlike the account store, `load` never errors: missing or corrupt means
//! defaults.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const DEFAULT_REFRESH_MINUTES: u64 = 5;
pub const MIN_REFRESH_MINUTES: u64 = 1;
pub const MAX_REFRESH_MINUTES: u64 = 60;

fn default_true() -> bool {
    true
}

fn default_minutes() -> u64 {
    DEFAULT_REFRESH_MINUTES
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawSettings")]
pub struct Settings {
    #[serde(rename = "autoRefreshEnabled")]
    pub auto_refresh_enabled: bool,
    #[serde(rename = "autoRefreshMinutes")]
    pub auto_refresh_minutes: u64,
    #[serde(rename = "preferredScanBrowser", skip_serializing_if = "Option::is_none")]
    pub preferred_scan_browser: Option<String>,
    #[serde(rename = "launchAtStartup")]
    pub launch_at_startup: bool,
    #[serde(rename = "autoUpdate")]
    pub auto_update: bool,
    #[serde(rename = "lastAutoUpdateCheck", skip_serializing_if = "Option::is_none")]
    pub last_auto_update_check_unix: Option<f64>,
    /// macOS `hasLaunchedBefore`: the main window opens by itself only on the first launch.
    #[serde(rename = "hasLaunchedBefore")]
    pub has_launched_before: bool,
}

/// On-disk shape: also accepts the legacy `autoRefreshSeconds` and `shell`
/// keys, which are migrated or dropped on load and never written back.
#[derive(Deserialize)]
struct RawSettings {
    #[serde(rename = "autoRefreshEnabled", default = "default_true")]
    auto_refresh_enabled: bool,
    #[serde(rename = "autoRefreshMinutes", default)]
    auto_refresh_minutes: Option<u64>,
    #[serde(rename = "autoRefreshSeconds", default)]
    auto_refresh_seconds: Option<u64>,
    #[serde(rename = "preferredScanBrowser", default)]
    preferred_scan_browser: Option<String>,
    #[serde(rename = "launchAtStartup", default)]
    launch_at_startup: bool,
    #[serde(rename = "autoUpdate", default = "default_true")]
    auto_update: bool,
    #[serde(rename = "lastAutoUpdateCheck", default)]
    last_auto_update_check_unix: Option<f64>,
    #[serde(rename = "hasLaunchedBefore", default)]
    has_launched_before: bool,
}

impl From<RawSettings> for Settings {
    fn from(r: RawSettings) -> Self {
        let minutes = match (r.auto_refresh_minutes, r.auto_refresh_seconds) {
            (Some(m), _) => m,
            (None, Some(0)) | (None, None) => default_minutes(),
            (None, Some(s)) => (s.saturating_add(30) / 60).clamp(MIN_REFRESH_MINUTES, MAX_REFRESH_MINUTES),
        };
        Settings {
            auto_refresh_enabled: r.auto_refresh_enabled,
            auto_refresh_minutes: minutes,
            preferred_scan_browser: r.preferred_scan_browser,
            launch_at_startup: r.launch_at_startup,
            auto_update: r.auto_update,
            last_auto_update_check_unix: r.last_auto_update_check_unix,
            has_launched_before: r.has_launched_before,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auto_refresh_enabled: true,
            auto_refresh_minutes: DEFAULT_REFRESH_MINUTES,
            preferred_scan_browser: None,
            launch_at_startup: false,
            auto_update: true,
            last_auto_update_check_unix: None,
            has_launched_before: false,
        }
    }
}

impl Settings {
    /// Refresh interval to use, clamped to [1, 60] minutes.
    pub fn effective_refresh_minutes(&self) -> u64 {
        self.auto_refresh_minutes.clamp(MIN_REFRESH_MINUTES, MAX_REFRESH_MINUTES)
    }
}

/// Settings > General "Reset": auto refresh back to on / 5 min and auto update
/// on. Launch at startup, the scan browser and the update-check stamp stay.
pub fn reset_general(s: &mut Settings) {
    s.auto_refresh_enabled = true;
    s.auto_refresh_minutes = DEFAULT_REFRESH_MINUTES;
    s.auto_update = true;
}

/// `settings.json` beside `accounts.json`.
pub fn settings_path() -> PathBuf {
    crate::store::accounts_path().with_file_name("settings.json")
}

/// Never fails: a missing or unparseable file yields the defaults.
pub fn load() -> Settings {
    load_from(&settings_path())
}

fn load_from(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

/// Writes via a sibling temp file then `rename`, so a crash never leaves a
/// half-written file.
pub fn save(s: &Settings) -> Result<(), String> {
    save_to(&settings_path(), s)
}

fn save_to(path: &Path, s: &Settings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    write_atomic(path, json.as_bytes())
}

static RMW: Mutex<()> = Mutex::new(());

/// Load, modify and save under one process-wide lock, so concurrent setters
/// (separate worker threads) cannot drop each other's write.
pub fn update(f: impl FnOnce(&mut Settings)) -> Result<(), String> {
    update_at(&settings_path(), f)
}

fn update_at(path: &Path, f: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let _guard = RMW.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load_from(path);
    f(&mut s);
    save_to(path, &s)
}

/// Temp file + `rename` into place (creating parent dirs); shared with the
/// other small JSON stores.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("json.{}.{unique}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    result.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

// Tests use explicit paths rather than APPDATA: `store.rs` guards the same
// process-global variables with its own private lock, which this module
// cannot share, so touching the environment here would race its tests.
#[cfg(test)]
mod tests {
    use super::*;

    fn with(minutes: u64) -> Settings {
        Settings { auto_refresh_minutes: minutes, ..Settings::default() }
    }

    fn load_str(json: &str) -> Settings {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, json).unwrap();
        load_from(&p)
    }

    #[test]
    fn has_launched_before_defaults_false_round_trips_and_survives_reset() {
        assert!(!load_str("{}").has_launched_before);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        update_at(&p, |s| s.has_launched_before = true).unwrap();
        assert!(load_from(&p).has_launched_before);
        assert!(fs::read_to_string(&p).unwrap().contains("\"hasLaunchedBefore\": true"));
        update_at(&p, reset_general).unwrap();
        assert!(load_from(&p).has_launched_before, "Reset keeps it");
    }

    #[test]
    fn path_is_settings_json_beside_accounts() {
        assert_eq!(settings_path().file_name().unwrap(), "settings.json");
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = load_from(&dir.path().join("settings.json"));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn defaults_are_macos() {
        let s = Settings::default();
        assert!(s.auto_refresh_enabled);
        assert_eq!(s.auto_refresh_minutes, 5);
    }

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested").join("settings.json");
        let s = Settings {
            auto_refresh_enabled: false,
            auto_refresh_minutes: 15,
            preferred_scan_browser: Some("edge".into()),
            launch_at_startup: true,
            auto_update: true,
            last_auto_update_check_unix: None,
            has_launched_before: true,
        };
        save_to(&p, &s).unwrap();
        assert_eq!(load_from(&p), s);
        assert_eq!(fs::read_dir(p.parent().unwrap()).unwrap().count(), 1, "no temp litter");
    }

    #[test]
    fn legacy_seconds_migrate_to_minutes() {
        let s = load_str(r#"{"autoRefreshSeconds":45,"shell":"pwsh"}"#);
        assert!(s.auto_refresh_enabled);
        assert_eq!(s.auto_refresh_minutes, 1);
        assert_eq!(load_str(r#"{"autoRefreshSeconds":90}"#).auto_refresh_minutes, 2);
        assert_eq!(load_str(r#"{"autoRefreshSeconds":99999}"#).auto_refresh_minutes, 60);
        assert_eq!(load_str(r#"{"autoRefreshSeconds":0}"#).auto_refresh_minutes, 5);
    }

    #[test]
    fn migrated_save_drops_legacy_keys() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, r#"{"autoRefreshSeconds":45,"shell":"pwsh"}"#).unwrap();
        save_to(&p, &load_from(&p)).unwrap();
        let raw = fs::read_to_string(&p).unwrap();
        assert!(!raw.contains("autoRefreshSeconds"), "{raw}");
        assert!(!raw.contains("shell"), "{raw}");
        assert!(raw.contains("autoRefreshMinutes"), "{raw}");
    }

    #[test]
    fn new_keys_win_over_legacy() {
        let s = load_str(r#"{"autoRefreshMinutes":10,"autoRefreshSeconds":45}"#);
        assert_eq!(s.auto_refresh_minutes, 10);
    }

    #[test]
    fn auto_update_defaults_on_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, r#"{"autoRefreshMinutes":5}"#).unwrap();
        let s = load_from(&p);
        assert!(s.auto_update, "missing key -> on (macOS default)");
        assert_eq!(s.last_auto_update_check_unix, None);
        let s2 = Settings { auto_update: false, last_auto_update_check_unix: Some(123.0), ..Settings::default() };
        save_to(&p, &s2).unwrap();
        assert_eq!(load_from(&p), s2);
    }

    #[test]
    fn effective_minutes_clamps() {
        assert_eq!(with(0).effective_refresh_minutes(), 1);
        assert_eq!(with(61).effective_refresh_minutes(), 60);
        assert_eq!(with(7).effective_refresh_minutes(), 7);
    }

    #[test]
    fn reset_general_keeps_launch_and_scan_browser() {
        let mut s = Settings {
            auto_refresh_enabled: false,
            auto_refresh_minutes: 30,
            preferred_scan_browser: Some("edge".into()),
            launch_at_startup: true,
            auto_update: false,
            last_auto_update_check_unix: Some(9.0),
            has_launched_before: true,
        };
        reset_general(&mut s);
        assert!(s.auto_refresh_enabled);
        assert_eq!(s.auto_refresh_minutes, 5);
        assert!(s.auto_update);
        assert!(s.launch_at_startup);
        assert_eq!(s.preferred_scan_browser.as_deref(), Some("edge"));
        assert_eq!(s.last_auto_update_check_unix, Some(9.0));
    }

    #[test]
    fn update_at_persists_and_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        update_at(&p, |s| s.launch_at_startup = true).unwrap();
        update_at(&p, |s| s.auto_refresh_minutes = 15).unwrap();
        let s = load_from(&p);
        assert!(s.launch_at_startup);
        assert_eq!(s.auto_refresh_minutes, 15);
    }

    #[test]
    fn corrupt_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, "{ not json").unwrap();
        assert_eq!(load_from(&p), Settings::default());
    }
}
