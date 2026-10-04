//! Settings > General glue: Auto Refresh and Launch at startup. Plain blocking
//! functions; the UI calls them from a worker thread (registry / file I/O).

use claude_dashboard_core::{settings, startup};

pub const APP_NAME: &str = "Claude Dashboard";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Persists the Auto Refresh interval. The refresh loop re-reads settings each
/// cycle, so the new value applies from the next wait.
pub fn set_auto_refresh(seconds: u64) -> Result<(), String> {
    let mut s = settings::load();
    s.auto_refresh_seconds = seconds;
    settings::save(&s)
}

/// The interval to show in the UI (clamped, never 0).
pub fn current_auto_refresh() -> u64 {
    settings::load().effective_refresh_seconds()
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
    let mut s = settings::load();
    s.launch_at_startup = on;
    settings::save(&s)
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

        set_auto_refresh(300).unwrap();
        assert_eq!(settings::load().auto_refresh_seconds, 300);
        assert_eq!(current_auto_refresh(), 300);
        set_auto_refresh(5).unwrap();
        assert_eq!(current_auto_refresh(), 30);
    }
}
