//! Reads `$XDG_CONFIG_HOME/claude-dashboard/config.json`, the single
//! configuration file shared by the daemon and the GTK app.
//!
//! Deliberately not GSettings: the helper must not take a `glib`/`gio`
//! dependency, which would end its pure-Rust dependency list and its static
//! build. See the spec's "config.json" section.

use serde::Deserialize;
use std::path::PathBuf;

/// A poll cadence below this would hammer the API for no benefit, so a
/// nonsense value in the file is clamped rather than obeyed.
const MIN_INTERVAL_SECONDS: u64 = 30;
const DEFAULT_INTERVAL_SECONDS: u64 = 120;

#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    pub refresh_interval_seconds: u64,
    pub auto_refresh_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval_seconds: DEFAULT_INTERVAL_SECONDS,
            auto_refresh_enabled: true,
        }
    }
}

/// serde's `deny_unknown_fields` is deliberately NOT used: the app may write
/// keys a older daemon has never heard of, and that must not disable polling.
#[derive(Deserialize)]
struct Wire {
    #[serde(rename = "refreshIntervalSeconds")]
    refresh_interval_seconds: Option<u64>,
    #[serde(rename = "autoRefreshEnabled")]
    auto_refresh_enabled: Option<bool>,
}

pub fn parse_config(text: &str) -> Config {
    let Ok(wire) = serde_json::from_str::<Wire>(text) else {
        return Config::default();
    };
    let interval = wire
        .refresh_interval_seconds
        .unwrap_or(DEFAULT_INTERVAL_SECONDS)
        .max(MIN_INTERVAL_SECONDS);
    Config {
        refresh_interval_seconds: interval,
        auto_refresh_enabled: wire.auto_refresh_enabled.unwrap_or(true),
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("claude-dashboard").join("config.json")
}

/// A missing file is not an error: it means "no preferences set yet".
pub fn load_config() -> Config {
    match std::fs::read_to_string(config_path()) {
        Ok(text) => parse_config(&text),
        Err(_) => Config::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_content_is_all_defaults() {
        let c = parse_config("");
        assert_eq!(c, Config::default());
        assert_eq!(c.refresh_interval_seconds, 120);
        assert!(c.auto_refresh_enabled);
    }

    #[test]
    fn malformed_json_falls_back_to_defaults_rather_than_failing() {
        assert_eq!(parse_config("{not json"), Config::default());
    }

    #[test]
    fn known_keys_are_read() {
        let c = parse_config(r#"{"refreshIntervalSeconds":300,"autoRefreshEnabled":false}"#);
        assert_eq!(c.refresh_interval_seconds, 300);
        assert!(!c.auto_refresh_enabled);
    }

    #[test]
    fn unknown_keys_are_ignored_not_rejected() {
        let c = parse_config(r#"{"refreshIntervalSeconds":60,"somethingNew":42}"#);
        assert_eq!(c.refresh_interval_seconds, 60);
        assert!(c.auto_refresh_enabled);
    }

    #[test]
    fn a_zero_interval_is_clamped_to_the_floor() {
        assert_eq!(parse_config(r#"{"refreshIntervalSeconds":0}"#).refresh_interval_seconds, 30);
    }
}
