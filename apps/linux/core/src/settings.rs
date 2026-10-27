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

// Auto Switch and Critical Accounts (macOS `AutoSwitchSettings` / `CriticalSettings`).
pub const AUTO_SWITCH_FIVE_HOUR_RANGE: std::ops::RangeInclusive<i64> = 50..=99;
pub const AUTO_SWITCH_SEVEN_DAY_RANGE: std::ops::RangeInclusive<i64> = 50..=100;
pub const DEFAULT_AUTO_SWITCH_FIVE_HOUR: i64 = 95;
pub const DEFAULT_AUTO_SWITCH_SEVEN_DAY: i64 = 99;
pub const CRITICAL_PACE_RANGE: std::ops::RangeInclusive<i64> = 10..=100;
pub const CRITICAL_PACE_STEP: i64 = 5;
pub const CRITICAL_HOUR_RANGE: std::ops::RangeInclusive<i64> = 0..=23;
pub const DEFAULT_CRITICAL_PACE: i64 = 50;
pub const DEFAULT_CRITICAL_HIGH_PACE: i64 = 80;
pub const DEFAULT_CRITICAL_QUIET_START: i64 = 0;
pub const DEFAULT_CRITICAL_QUIET_END: i64 = 7;
pub const DEFAULT_CRITICAL_TIME_ZONE: &str = "Asia/Ho_Chi_Minh";

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
    /// macOS `autoPingEnabled`: ping every active account whose 5h or 7d window is not running.
    #[serde(rename = "autoPingEnabled")]
    pub auto_ping_enabled: bool,
    /// macOS `switchDesktopEnabled`: the Switch button also moves Claude Desktop.
    #[serde(rename = "switchDesktopEnabled")]
    pub switch_desktop_enabled: bool,
    /// macOS `autoSwitchDesktopEnabled`: auto switch also moves Claude Desktop.
    #[serde(rename = "autoSwitchDesktopEnabled")]
    pub auto_switch_desktop_enabled: bool,
    /// macOS `showBurnAnimals`: show the burn-rate animal on each gauge and in the Overview
    /// legend. Display only: the burn-rate tracker and the sort order run either way.
    #[serde(rename = "showBurnAnimals")]
    pub show_burn_animals: bool,
    #[serde(rename = "autoSwitchEnabled")]
    pub auto_switch_enabled: bool,
    #[serde(rename = "autoSwitchFiveHourThreshold")]
    pub auto_switch_five_hour: i64,
    #[serde(rename = "autoSwitchSevenDayThreshold")]
    pub auto_switch_seven_day: i64,
    /// Lets the flyout footer show the toggle when the active account nears a limit.
    #[serde(rename = "autoSwitchShowWidgetToggle")]
    pub auto_switch_show_widget_toggle: bool,
    /// The home account (bolt): Claude Code comes back to it after each of its 5h resets.
    #[serde(rename = "autoSwitchPriorityAccountId", skip_serializing_if = "Option::is_none")]
    pub auto_switch_priority_id: Option<String>,
    /// Its 5h `resetsAt` (Unix seconds) when ticked or last used; none before its window started.
    #[serde(rename = "autoSwitchPriorityBaseline", skip_serializing_if = "Option::is_none")]
    pub auto_switch_priority_baseline: Option<f64>,
    #[serde(rename = "criticalPace")]
    pub critical_pace: i64,
    /// Never below `critical_pace`.
    #[serde(rename = "criticalHighPace")]
    pub critical_high_pace: i64,
    #[serde(rename = "criticalQuietStart")]
    pub critical_quiet_start: i64,
    #[serde(rename = "criticalQuietEnd")]
    pub critical_quiet_end: i64,
    /// An IANA zone name the quiet hours are read in.
    #[serde(rename = "criticalTimeZone")]
    pub critical_time_zone: String,
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
    #[serde(rename = "autoPingEnabled", default)]
    auto_ping_enabled: bool,
    #[serde(rename = "switchDesktopEnabled", default)]
    switch_desktop_enabled: bool,
    #[serde(rename = "autoSwitchDesktopEnabled", default)]
    auto_switch_desktop_enabled: bool,
    #[serde(rename = "showBurnAnimals", default)]
    show_burn_animals: bool,
    #[serde(rename = "autoSwitchEnabled", default = "default_true")]
    auto_switch_enabled: bool,
    #[serde(rename = "autoSwitchFiveHourThreshold", default, deserialize_with = "lenient_i64")]
    auto_switch_five_hour: Option<i64>,
    #[serde(rename = "autoSwitchSevenDayThreshold", default, deserialize_with = "lenient_i64")]
    auto_switch_seven_day: Option<i64>,
    #[serde(rename = "autoSwitchShowWidgetToggle", default = "default_true")]
    auto_switch_show_widget_toggle: bool,
    #[serde(rename = "autoSwitchPriorityAccountId", default)]
    auto_switch_priority_id: Option<String>,
    #[serde(rename = "autoSwitchPriorityBaseline", default)]
    auto_switch_priority_baseline: Option<f64>,
    #[serde(rename = "criticalPace", default, deserialize_with = "lenient_i64")]
    critical_pace: Option<i64>,
    #[serde(rename = "criticalHighPace", default, deserialize_with = "lenient_i64")]
    critical_high_pace: Option<i64>,
    #[serde(rename = "criticalQuietStart", default, deserialize_with = "lenient_i64")]
    critical_quiet_start: Option<i64>,
    #[serde(rename = "criticalQuietEnd", default, deserialize_with = "lenient_i64")]
    critical_quiet_end: Option<i64>,
    #[serde(rename = "criticalTimeZone", default)]
    critical_time_zone: Option<String>,
}

/// A number of the wrong type (a float, a string) reads as missing, for that key only
/// (macOS `as? Int`): one bad value never resets the whole file.
fn lenient_i64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(Option::<serde_json::Value>::deserialize(d)?.and_then(|v| v.as_i64()))
}

/// A stored value outside `range` (or missing) loads as `default` (macOS `stored(_:in:default:)`).
fn in_range(v: Option<i64>, range: std::ops::RangeInclusive<i64>, default: i64) -> i64 {
    v.filter(|v| range.contains(v)).unwrap_or(default)
}

impl From<RawSettings> for Settings {
    fn from(r: RawSettings) -> Self {
        let minutes = match (r.auto_refresh_minutes, r.auto_refresh_seconds) {
            (Some(m), _) => m,
            (None, Some(0)) | (None, None) => default_minutes(),
            (None, Some(s)) => (s.saturating_add(30) / 60).clamp(MIN_REFRESH_MINUTES, MAX_REFRESH_MINUTES),
        };
        let pace = in_range(r.critical_pace, CRITICAL_PACE_RANGE, DEFAULT_CRITICAL_PACE);
        Settings {
            auto_refresh_enabled: r.auto_refresh_enabled,
            auto_refresh_minutes: minutes,
            preferred_scan_browser: r.preferred_scan_browser,
            launch_at_startup: r.launch_at_startup,
            auto_update: r.auto_update,
            last_auto_update_check_unix: r.last_auto_update_check_unix,
            has_launched_before: r.has_launched_before,
            auto_ping_enabled: r.auto_ping_enabled,
            switch_desktop_enabled: r.switch_desktop_enabled,
            auto_switch_desktop_enabled: r.auto_switch_desktop_enabled,
            show_burn_animals: r.show_burn_animals,
            auto_switch_enabled: r.auto_switch_enabled,
            auto_switch_five_hour: in_range(r.auto_switch_five_hour, AUTO_SWITCH_FIVE_HOUR_RANGE, DEFAULT_AUTO_SWITCH_FIVE_HOUR),
            auto_switch_seven_day: in_range(r.auto_switch_seven_day, AUTO_SWITCH_SEVEN_DAY_RANGE, DEFAULT_AUTO_SWITCH_SEVEN_DAY),
            auto_switch_show_widget_toggle: r.auto_switch_show_widget_toggle,
            auto_switch_priority_id: r.auto_switch_priority_id,
            auto_switch_priority_baseline: r.auto_switch_priority_baseline,
            critical_pace: pace,
            critical_high_pace: in_range(r.critical_high_pace, CRITICAL_PACE_RANGE, DEFAULT_CRITICAL_HIGH_PACE).max(pace),
            critical_quiet_start: in_range(r.critical_quiet_start, CRITICAL_HOUR_RANGE, DEFAULT_CRITICAL_QUIET_START),
            critical_quiet_end: in_range(r.critical_quiet_end, CRITICAL_HOUR_RANGE, DEFAULT_CRITICAL_QUIET_END),
            critical_time_zone: r
                .critical_time_zone
                .filter(|z| z.parse::<chrono_tz::Tz>().is_ok())
                .unwrap_or_else(|| DEFAULT_CRITICAL_TIME_ZONE.to_string()),
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
            auto_ping_enabled: false,
            switch_desktop_enabled: false,
            auto_switch_desktop_enabled: false,
            show_burn_animals: false,
            auto_switch_enabled: true,
            auto_switch_five_hour: DEFAULT_AUTO_SWITCH_FIVE_HOUR,
            auto_switch_seven_day: DEFAULT_AUTO_SWITCH_SEVEN_DAY,
            auto_switch_show_widget_toggle: true,
            auto_switch_priority_id: None,
            auto_switch_priority_baseline: None,
            critical_pace: DEFAULT_CRITICAL_PACE,
            critical_high_pace: DEFAULT_CRITICAL_HIGH_PACE,
            critical_quiet_start: DEFAULT_CRITICAL_QUIET_START,
            critical_quiet_end: DEFAULT_CRITICAL_QUIET_END,
            critical_time_zone: DEFAULT_CRITICAL_TIME_ZONE.to_string(),
        }
    }
}

impl Settings {
    /// Refresh interval to use, clamped to [1, 60] minutes.
    pub fn effective_refresh_minutes(&self) -> u64 {
        self.auto_refresh_minutes.clamp(MIN_REFRESH_MINUTES, MAX_REFRESH_MINUTES)
    }

    /// Raising it above the high critical pace raises that with it (macOS `pace.didSet`).
    pub fn set_critical_pace(&mut self, v: i64) {
        self.critical_pace = v.clamp(*CRITICAL_PACE_RANGE.start(), *CRITICAL_PACE_RANGE.end());
        self.critical_high_pace = self.critical_high_pace.max(self.critical_pace);
    }

    /// Never below the critical pace.
    pub fn set_critical_high_pace(&mut self, v: i64) {
        self.critical_high_pace = v.clamp(self.critical_pace, *CRITICAL_PACE_RANGE.end());
    }
}

/// Settings > General "Reset": auto refresh back to on / 5 min, Auto Ping and burn-rate animals off, Auto Switch
/// and Critical Accounts back to their defaults, auto update on. Launch at startup, the scan
/// browser, the update-check stamp and the home account stay (macOS `resetSettingsToDefaults`).
pub fn reset_general(s: &mut Settings) {
    let d = Settings::default();
    s.auto_refresh_enabled = true;
    s.auto_refresh_minutes = DEFAULT_REFRESH_MINUTES;
    s.auto_ping_enabled = false;
    s.switch_desktop_enabled = false;
    s.auto_switch_desktop_enabled = false;
    s.show_burn_animals = false;
    s.auto_update = true;
    s.auto_switch_enabled = d.auto_switch_enabled;
    s.auto_switch_five_hour = d.auto_switch_five_hour;
    s.auto_switch_seven_day = d.auto_switch_seven_day;
    s.auto_switch_show_widget_toggle = d.auto_switch_show_widget_toggle;
    s.critical_pace = d.critical_pace;
    s.critical_high_pace = d.critical_high_pace;
    s.critical_quiet_start = d.critical_quiet_start;
    s.critical_quiet_end = d.critical_quiet_end;
    s.critical_time_zone = d.critical_time_zone;
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
    fn desktop_keys_default_off() {
        let s = load_str("{}");
        assert!(!s.switch_desktop_enabled && !s.auto_switch_desktop_enabled);
    }

    #[test]
    fn desktop_keys_round_trip_with_macos_names() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        update_at(&p, |s| {
            s.switch_desktop_enabled = true;
            s.auto_switch_desktop_enabled = true;
        })
        .unwrap();
        let json = fs::read_to_string(&p).unwrap();
        assert!(json.contains("\"switchDesktopEnabled\": true"), "{json}");
        assert!(json.contains("\"autoSwitchDesktopEnabled\": true"), "{json}");
        let s = load_from(&p);
        assert!(s.switch_desktop_enabled && s.auto_switch_desktop_enabled);
    }

    #[test]
    fn reset_turns_desktop_keys_off() {
        let mut s = Settings { switch_desktop_enabled: true, auto_switch_desktop_enabled: true, ..Settings::default() };
        reset_general(&mut s);
        assert!(!s.switch_desktop_enabled && !s.auto_switch_desktop_enabled);
    }

    #[test]
    fn auto_ping_defaults_off_round_trips_and_reset_turns_off() {
        assert!(!load_str("{}").auto_ping_enabled);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        update_at(&p, |s| s.auto_ping_enabled = true).unwrap();
        assert!(load_from(&p).auto_ping_enabled);
        assert!(fs::read_to_string(&p).unwrap().contains("autoPingEnabled"));
        update_at(&p, reset_general).unwrap();
        assert!(!load_from(&p).auto_ping_enabled, "Reset turns Auto Ping off (macOS)");
    }

    #[test]
    fn show_burn_animals_defaults_off_round_trips_and_reset_turns_off() {
        assert!(!load_str("{}").show_burn_animals);
        assert!(!Settings::default().show_burn_animals);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        update_at(&p, |s| s.show_burn_animals = true).unwrap();
        assert!(load_from(&p).show_burn_animals);
        assert!(fs::read_to_string(&p).unwrap().contains("\"showBurnAnimals\": true"));
        update_at(&p, reset_general).unwrap();
        assert!(!load_from(&p).show_burn_animals, "Reset turns burn-rate animals off (macOS)");
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
            auto_ping_enabled: true,
            ..Settings::default()
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
            auto_ping_enabled: true,
            ..Settings::default()
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
    fn auto_switch_and_critical_defaults() {
        let s = load_str("{}");
        assert!(s.auto_switch_enabled, "macOS: on by default");
        assert_eq!((s.auto_switch_five_hour, s.auto_switch_seven_day), (95, 99));
        assert!(s.auto_switch_show_widget_toggle);
        assert_eq!((s.auto_switch_priority_id.clone(), s.auto_switch_priority_baseline), (None, None));
        assert_eq!((s.critical_pace, s.critical_high_pace), (50, 80));
        assert_eq!((s.critical_quiet_start, s.critical_quiet_end), (0, 7));
        assert_eq!(s.critical_time_zone, "Asia/Ho_Chi_Minh");
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn out_of_range_values_fall_back() {
        let s = load_str(
            r#"{"autoSwitchFiveHourThreshold":100,"autoSwitchSevenDayThreshold":49,"criticalPace":5,
                "criticalHighPace":101,"criticalQuietStart":24,"criticalQuietEnd":-1,"criticalTimeZone":"Mars/Base"}"#,
        );
        assert_eq!((s.auto_switch_five_hour, s.auto_switch_seven_day), (95, 99));
        assert_eq!((s.critical_pace, s.critical_high_pace), (50, 80));
        assert_eq!((s.critical_quiet_start, s.critical_quiet_end), (0, 7));
        assert_eq!(s.critical_time_zone, "Asia/Ho_Chi_Minh");
        let edge = load_str(r#"{"autoSwitchFiveHourThreshold":50,"autoSwitchSevenDayThreshold":100,"criticalTimeZone":"Europe/Paris"}"#);
        assert_eq!((edge.auto_switch_five_hour, edge.auto_switch_seven_day), (50, 100));
        assert_eq!(edge.critical_time_zone, "Europe/Paris");
    }

    #[test]
    fn high_pace_never_below_pace() {
        let s = load_str(r#"{"criticalPace":90,"criticalHighPace":60}"#);
        assert_eq!((s.critical_pace, s.critical_high_pace), (90, 90));
        let mut s = Settings::default();
        s.set_critical_pace(95);
        assert_eq!((s.critical_pace, s.critical_high_pace), (95, 95), "raising pace raises high pace");
        s.set_critical_high_pace(60);
        assert_eq!(s.critical_high_pace, 95, "high pace never drops below pace");
        s.set_critical_pace(10);
        assert_eq!((s.critical_pace, s.critical_high_pace), (10, 95));
    }

    #[test]
    fn reset_general_resets_switch_and_critical_but_keeps_home_account() {
        let mut s = Settings {
            auto_switch_enabled: false,
            auto_switch_five_hour: 60,
            auto_switch_seven_day: 70,
            auto_switch_show_widget_toggle: false,
            auto_switch_priority_id: Some("home".into()),
            auto_switch_priority_baseline: Some(42.0),
            critical_pace: 20,
            critical_high_pace: 30,
            critical_quiet_start: 22,
            critical_quiet_end: 6,
            critical_time_zone: "Europe/Paris".into(),
            ..Settings::default()
        };
        reset_general(&mut s);
        let d = Settings::default();
        assert_eq!(
            (s.auto_switch_enabled, s.auto_switch_five_hour, s.auto_switch_seven_day, s.auto_switch_show_widget_toggle),
            (true, 95, 99, true)
        );
        assert_eq!((s.critical_pace, s.critical_high_pace, s.critical_quiet_start, s.critical_quiet_end), (50, 80, 0, 7));
        assert_eq!(s.critical_time_zone, d.critical_time_zone);
        assert_eq!((s.auto_switch_priority_id.as_deref(), s.auto_switch_priority_baseline), (Some("home"), Some(42.0)));
    }

    #[test]
    fn round_trip_keys_use_macos_names() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s = Settings {
            auto_switch_enabled: false,
            auto_switch_five_hour: 90,
            auto_switch_seven_day: 98,
            auto_switch_show_widget_toggle: false,
            auto_switch_priority_id: Some("abc".into()),
            auto_switch_priority_baseline: Some(1_700_000_000.0),
            critical_pace: 55,
            critical_high_pace: 85,
            critical_quiet_start: 23,
            critical_quiet_end: 5,
            critical_time_zone: "Europe/Paris".into(),
            ..Settings::default()
        };
        save_to(&p, &s).unwrap();
        assert_eq!(load_from(&p), s);
        let raw = fs::read_to_string(&p).unwrap();
        for key in [
            "autoSwitchEnabled", "autoSwitchFiveHourThreshold", "autoSwitchSevenDayThreshold", "autoSwitchShowWidgetToggle",
            "autoSwitchPriorityAccountId", "autoSwitchPriorityBaseline", "criticalPace", "criticalHighPace",
            "criticalQuietStart", "criticalQuietEnd", "criticalTimeZone",
        ] {
            assert!(raw.contains(key), "{key} missing in {raw}");
        }
        save_to(&p, &Settings::default()).unwrap();
        let raw = fs::read_to_string(&p).unwrap();
        assert!(!raw.contains("autoSwitchPriorityAccountId"), "no home account -> key absent: {raw}");
    }

    /// macOS reads each key with `as? Int`: a value of the wrong type falls back for that key
    /// only, never for the whole file (the home account and the other keys are kept).
    #[test]
    fn a_non_integer_value_falls_back_for_that_key_only() {
        let s = load_str(r#"{"autoSwitchFiveHourThreshold":95.5,"autoSwitchSevenDayThreshold":"x","criticalPace":60,
            "criticalQuietEnd":true,"autoSwitchPriorityAccountId":"home","autoRefreshMinutes":12}"#);
        assert_eq!((s.auto_switch_five_hour, s.auto_switch_seven_day), (95, 99));
        assert_eq!((s.critical_pace, s.critical_quiet_end), (60, 7));
        assert_eq!(s.auto_switch_priority_id.as_deref(), Some("home"));
        assert_eq!(s.auto_refresh_minutes, 12);
    }

    #[test]
    fn corrupt_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, "{ not json").unwrap();
        assert_eq!(load_from(&p), Settings::default());
    }
}
