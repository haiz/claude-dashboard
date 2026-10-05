//! Non-critical app settings (`settings.json`, next to `accounts.json`).
//! Unlike the account store, `load` never errors: missing or corrupt means
//! defaults.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const DEFAULT_REFRESH_SECONDS: u64 = 60;
pub const MIN_REFRESH_SECONDS: u64 = 30;
pub const MAX_REFRESH_SECONDS: u64 = 3600;

fn default_refresh() -> u64 {
    DEFAULT_REFRESH_SECONDS
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "autoRefreshSeconds", default = "default_refresh")]
    pub auto_refresh_seconds: u64,
    #[serde(rename = "preferredScanBrowser", skip_serializing_if = "Option::is_none", default)]
    pub preferred_scan_browser: Option<String>,
    #[serde(rename = "launchAtStartup", default)]
    pub launch_at_startup: bool,
    #[serde(rename = "shell", skip_serializing_if = "Option::is_none", default)]
    pub shell: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auto_refresh_seconds: DEFAULT_REFRESH_SECONDS,
            preferred_scan_browser: None,
            launch_at_startup: false,
            shell: None,
        }
    }
}

impl Settings {
    /// Refresh interval to use: clamped to [30, 3600]; 0 means unset -> 60.
    pub fn effective_refresh_seconds(&self) -> u64 {
        match self.auto_refresh_seconds {
            0 => DEFAULT_REFRESH_SECONDS,
            n => n.clamp(MIN_REFRESH_SECONDS, MAX_REFRESH_SECONDS),
        }
    }
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

    fn with(secs: u64) -> Settings {
        Settings { auto_refresh_seconds: secs, ..Settings::default() }
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
        assert_eq!(s.auto_refresh_seconds, 60);
    }

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested").join("settings.json");
        let s = Settings {
            auto_refresh_seconds: 120,
            preferred_scan_browser: Some("edge".into()),
            launch_at_startup: true,
            shell: Some("pwsh".into()),
        };
        save_to(&p, &s).unwrap();
        assert_eq!(load_from(&p), s);
        assert_eq!(fs::read_dir(p.parent().unwrap()).unwrap().count(), 1, "no temp litter");
    }

    #[test]
    fn missing_shell_key_loads_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, r#"{"autoRefreshSeconds":60}"#).unwrap();
        assert_eq!(load_from(&p).shell, None);
    }

    #[test]
    fn effective_refresh_clamps() {
        assert_eq!(with(0).effective_refresh_seconds(), 60);
        assert_eq!(with(5).effective_refresh_seconds(), 30);
        assert_eq!(with(99999).effective_refresh_seconds(), 3600);
        assert_eq!(with(120).effective_refresh_seconds(), 120);
    }

    #[test]
    fn update_at_persists_and_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        update_at(&p, |s| s.shell = Some("cmd".into())).unwrap();
        update_at(&p, |s| s.auto_refresh_seconds = 300).unwrap();
        let s = load_from(&p);
        assert_eq!(s.shell.as_deref(), Some("cmd"));
        assert_eq!(s.auto_refresh_seconds, 300);
    }

    #[test]
    fn corrupt_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, "{ not json").unwrap();
        assert_eq!(load_from(&p), Settings::default());
    }
}
