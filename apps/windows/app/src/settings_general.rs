//! Settings > General glue: Auto Refresh and Launch at startup. Plain blocking
//! functions; the UI calls them from a worker thread (registry / file I/O).

use claude_dashboard_core::{settings, startup};

pub const APP_NAME: &str = "Claude Dashboard";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Persists the Auto Refresh interval. The refresh loop re-reads settings each
/// cycle, so the new value applies from the next wait.
pub fn set_auto_refresh(seconds: u64) -> Result<(), String> {
    settings::update(|s| s.auto_refresh_seconds = seconds)
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
    settings::update(|s| s.launch_at_startup = on)
}

/// (key, label) for each shell installed on this machine.
pub fn shells() -> Vec<(String, String)> {
    crate::shell::detect_available()
        .into_iter()
        .map(|s| (s.kind.setting_key().to_string(), s.kind.label().to_string()))
        .collect()
}

/// The shell that runs will actually use (saved choice, else auto), or "".
pub fn current_shell_key() -> String {
    crate::shell::detect(settings::load().shell.as_deref())
        .map(|s| s.kind.setting_key().to_string())
        .unwrap_or_default()
}

/// Persists the shell choice; an unknown key is rejected.
pub fn set_shell(key: &str) -> Result<(), String> {
    if crate::shell::ShellKind::from_setting(key).is_none() {
        return Err(format!("unknown shell: {key}"));
    }
    settings::update(|s| s.shell = Some(key.to_string()))
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

    #[test]
    fn set_shell_persists_known_keys_only() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        set_shell("cmd").unwrap();
        assert_eq!(settings::load().shell.as_deref(), Some("cmd"));
        assert!(set_shell("zsh").is_err());
        assert_eq!(settings::load().shell.as_deref(), Some("cmd"));
        assert_eq!(current_shell_key(), "cmd", "cmd.exe always exists on Windows");
    }
}
