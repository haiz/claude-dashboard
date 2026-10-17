//! Settings > General glue: Auto Refresh and Launch at startup. Plain blocking
//! functions; the UI calls them from a worker thread (registry / file I/O).

use claude_dashboard_core::{settings, startup};

pub const APP_NAME: &str = "Claude Dashboard";

type Job = Box<dyn FnOnce() + Send>;

/// One worker thread that runs settings writes in the order they were
/// submitted, so rapid clicks (the interval stepper) cannot land out of order
/// the way one thread per click could.
pub struct SettingsWriter {
    tx: std::sync::Mutex<std::sync::mpsc::Sender<Job>>,
}

impl SettingsWriter {
    pub fn spawn() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::spawn(move || {
            for job in rx {
                job();
            }
        });
        SettingsWriter {
            tx: std::sync::Mutex::new(tx),
        }
    }

    /// Queues `job` behind every write submitted before it.
    pub fn submit(&self, job: impl FnOnce() + Send + 'static) {
        let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        let _ = tx.send(Box::new(job));
    }
}

/// The app's settings writer (Settings > General and the auto-update toggle).
pub fn writer() -> &'static SettingsWriter {
    static W: std::sync::LazyLock<SettingsWriter> = std::sync::LazyLock::new(SettingsWriter::spawn);
    &W
}
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

#[derive(Debug, PartialEq, Eq)]
pub struct GeneralState {
    pub auto_refresh_enabled: bool,
    pub auto_refresh_minutes: u64,
    pub auto_update: bool,
}

/// Build date of the running release (`yyyy-MM-dd`), "" for local builds. See `build.rs`.
pub const RELEASE_DATE: &str = env!("CLAUDE_DASHBOARD_RELEASE_DATE");

/// Settings > General "Reset All Settings": one atomic settings update, then the
/// values the UI shows. Accounts, logs, muted sources and Launch at startup stay.
pub fn reset_all() -> Result<GeneralState, String> {
    settings::update(settings::reset_general)?;
    let s = settings::load();
    Ok(GeneralState {
        auto_refresh_enabled: s.auto_refresh_enabled,
        auto_refresh_minutes: s.effective_refresh_minutes(),
        auto_update: s.auto_update,
    })
}

/// `yyyy-MM-dd` as macOS's abbreviated date ("Oct 17, 2026"); "" when empty or malformed.
pub fn format_release_date(raw: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let parts: Vec<&str> = raw.trim().split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return String::new();
    };
    let (Ok(y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<usize>(), d.parse::<u32>()) else {
        return String::new();
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y == 0 {
        return String::new();
    }
    format!("{} {d}, {y}", MONTHS[m - 1])
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
    fn writer_runs_jobs_in_submission_order() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        let w = SettingsWriter::spawn();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        for m in 1..=20u64 {
            let seen = seen.clone();
            w.submit(move || {
                set_auto_refresh_minutes(m).unwrap();
                seen.lock().unwrap().push(m);
            });
        }
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        w.submit(move || done_tx.send(()).unwrap());
        done_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), (1..=20).collect::<Vec<_>>());
        assert_eq!(current_auto_refresh(), (true, 20), "the last click wins");
    }

    #[test]
    fn reset_all_restores_refresh_and_update_keeps_startup() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        set_auto_refresh_minutes(30).unwrap();
        set_auto_refresh_enabled(false).unwrap();
        set_auto_update(false).unwrap();
        settings::update(|s| s.launch_at_startup = true).unwrap();

        let after = reset_all().unwrap();
        assert_eq!(
            after,
            GeneralState {
                auto_refresh_enabled: true,
                auto_refresh_minutes: 5,
                auto_update: true
            }
        );
        assert_eq!(current_auto_refresh(), (true, 5));
        assert!(auto_update_enabled());
        assert!(
            settings::load().launch_at_startup,
            "launch at startup is kept"
        );
    }

    #[test]
    fn release_date_formats_like_macos_abbreviated() {
        assert_eq!(format_release_date("2026-10-17"), "Oct 17, 2026");
        assert_eq!(format_release_date("2026-01-05"), "Jan 5, 2026");
        assert_eq!(format_release_date(""), "");
        assert_eq!(format_release_date("2026-13-01"), "");
        assert_eq!(format_release_date("garbage"), "");
    }

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
