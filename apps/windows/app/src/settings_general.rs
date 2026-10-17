//! Settings > General glue: Auto Refresh and Launch at startup. Plain blocking
//! functions; the UI calls them from a worker thread (registry / file I/O).

use claude_dashboard_core::{settings, startup};

pub const APP_NAME: &str = "Claude Dashboard";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Persists the Auto Refresh on/off switch. The refresh loop re-reads settings
/// each cycle (and on a nudge), so it applies from the next wait.
pub fn set_auto_refresh_enabled(on: bool) -> Result<(), String> {
    settings::update(|s| s.auto_refresh_enabled = on)
}

/// Persists the Auto Refresh interval in minutes.
pub fn set_auto_refresh_minutes(minutes: u64) -> Result<(), String> {
    settings::update(|s| s.auto_refresh_minutes = minutes)
}

/// `(enabled, minutes)` to show in the UI (minutes clamped to 1..=60).
pub fn current_auto_refresh() -> (bool, u64) {
    let s = settings::load();
    (s.auto_refresh_enabled, s.effective_refresh_minutes())
}

pub fn launch_is_enabled() -> bool {
    startup::is_enabled()
}

/// Flips the HKCU Run value, then mirrors the result into settings.
pub fn set_launch_at_startup(on: bool) -> Result<(), String> {
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        startup::enable(&exe.to_string_lossy())?;
    } else {
        startup::disable()?;
    }
    settings::update(|s| s.launch_at_startup = on)
}

pub fn auto_update_enabled() -> bool {
    settings::load().auto_update
}

pub fn set_auto_update(on: bool) -> Result<(), String> {
    settings::update(|s| s.auto_update = on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_auto_refresh_persists_and_clamps_for_display() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());

        assert_eq!(current_auto_refresh(), (true, 5));
        set_auto_refresh_minutes(15).unwrap();
        set_auto_refresh_enabled(false).unwrap();
        assert_eq!(settings::load().auto_refresh_minutes, 15);
        assert_eq!(current_auto_refresh(), (false, 15));
        set_auto_refresh_minutes(0).unwrap();
        assert_eq!(current_auto_refresh(), (false, 1));
    }

    #[test]
    fn auto_update_toggle_persists() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        assert!(auto_update_enabled(), "default on");
        set_auto_update(false).unwrap();
        assert!(!auto_update_enabled());
        assert!(!settings::load().auto_update);
    }
}
