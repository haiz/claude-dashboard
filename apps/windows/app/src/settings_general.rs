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
    pub auto_ping: bool,
    pub switch_desktop: bool,
    pub auto_switch_desktop: bool,
    pub show_burn_animals: bool,
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
        auto_ping: s.auto_ping_enabled,
        switch_desktop: s.switch_desktop_enabled,
        auto_switch_desktop: s.auto_switch_desktop_enabled,
        show_burn_animals: s.show_burn_animals,
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

/// Settings > General > Auto Switch and Critical Accounts, as the pane shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchState {
    pub on: bool,
    pub five: i64,
    pub seven: i64,
    pub widget: bool,
    pub pace: i64,
    pub high_pace: i64,
    pub quiet_start: i64,
    pub quiet_end: i64,
    pub zone: String,
}

pub fn switch_state(s: &settings::Settings) -> SwitchState {
    SwitchState {
        on: s.auto_switch_enabled,
        five: s.auto_switch_five_hour,
        seven: s.auto_switch_seven_day,
        widget: s.auto_switch_show_widget_toggle,
        pace: s.critical_pace,
        high_pace: s.critical_high_pace,
        quiet_start: s.critical_quiet_start,
        quiet_end: s.critical_quiet_end,
        zone: s.critical_time_zone.clone(),
    }
}

/// Applies one Auto Switch / Critical Accounts control: `key` is the pane's name for it and
/// `value` the new number (0 / 1 for a switch). Values are clamped to the macOS ranges, and
/// the high critical pace never drops below the critical pace.
pub fn apply_switch_setting(s: &mut settings::Settings, key: &str, value: i64) {
    let clamp = |r: std::ops::RangeInclusive<i64>| value.clamp(*r.start(), *r.end());
    match key {
        "on" => s.auto_switch_enabled = value != 0,
        "five" => s.auto_switch_five_hour = clamp(settings::AUTO_SWITCH_FIVE_HOUR_RANGE),
        "seven" => s.auto_switch_seven_day = clamp(settings::AUTO_SWITCH_SEVEN_DAY_RANGE),
        "widget" => s.auto_switch_show_widget_toggle = value != 0,
        "pace" => s.set_critical_pace(value),
        "high-pace" => s.set_critical_high_pace(value),
        "quiet-start" => s.critical_quiet_start = clamp(settings::CRITICAL_HOUR_RANGE),
        "quiet-end" => s.critical_quiet_end = clamp(settings::CRITICAL_HOUR_RANGE),
        _ => {}
    }
}

/// The time zone menu: a known IANA name, else nothing changes.
pub fn apply_critical_zone(s: &mut settings::Settings, zone: &str) {
    if claude_dashboard_core::critical::time_zone_names().contains(&zone) {
        s.critical_time_zone = zone.to_string();
    }
}

pub fn auto_ping_enabled() -> bool {
    settings::load().auto_ping_enabled
}

pub fn set_auto_ping(on: bool) -> Result<(), String> {
    settings::update(|s| s.auto_ping_enabled = on)
}

/// Settings > General > Claude Desktop: Switch also moves Claude Desktop. Read at use time.
pub fn switch_desktop_enabled() -> bool {
    settings::load().switch_desktop_enabled
}

pub fn set_switch_desktop(on: bool) -> Result<(), String> {
    settings::update(|s| s.switch_desktop_enabled = on)
}

/// Settings > General > Claude Desktop: auto switch also moves a closed Claude Desktop.
pub fn auto_switch_desktop_enabled() -> bool {
    settings::load().auto_switch_desktop_enabled
}

pub fn set_auto_switch_desktop(on: bool) -> Result<(), String> {
    settings::update(|s| s.auto_switch_desktop_enabled = on)
}

/// Settings > General "Show burn-rate animals" (default off). Display only.
pub fn show_burn_animals() -> bool {
    settings::load().show_burn_animals
}

pub fn set_show_burn_animals(on: bool) -> Result<(), String> {
    settings::update(|s| s.show_burn_animals = on)
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
    fn show_burn_animals_defaults_off_and_round_trips() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        assert!(!show_burn_animals(), "off by default");
        set_show_burn_animals(true).unwrap();
        assert!(show_burn_animals());
        set_show_burn_animals(false).unwrap();
        assert!(!show_burn_animals());
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
        set_auto_ping(true).unwrap();
        assert!(auto_ping_enabled());
        set_show_burn_animals(true).unwrap();
        assert!(show_burn_animals());
        settings::update(|s| s.launch_at_startup = true).unwrap();

        let after = reset_all().unwrap();
        assert_eq!(
            after,
            GeneralState {
                auto_refresh_enabled: true,
                auto_refresh_minutes: 5,
                auto_update: true,
                auto_ping: false,
                switch_desktop: false,
                auto_switch_desktop: false,
                show_burn_animals: false,
            }
        );
        assert!(!auto_ping_enabled());
        assert!(!show_burn_animals());
        assert_eq!(current_auto_refresh(), (true, 5));
        assert!(auto_update_enabled());
        assert!(
            settings::load().launch_at_startup,
            "launch at startup is kept"
        );
    }

    #[test]
    fn reset_all_turns_desktop_off() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        assert!(!switch_desktop_enabled() && !auto_switch_desktop_enabled(), "off by default");
        set_switch_desktop(true).unwrap();
        set_auto_switch_desktop(true).unwrap();
        assert!(switch_desktop_enabled() && auto_switch_desktop_enabled());
        let after = reset_all().unwrap();
        assert!(!after.switch_desktop && !after.auto_switch_desktop);
        assert!(!switch_desktop_enabled() && !auto_switch_desktop_enabled());
    }

    #[test]
    fn switch_settings_clamp_like_macos() {
        let mut s = settings::Settings::default();
        assert_eq!(
            switch_state(&s),
            SwitchState { on: true, five: 95, seven: 99, widget: true, pace: 50, high_pace: 80, quiet_start: 0, quiet_end: 7, zone: "Asia/Ho_Chi_Minh".into() }
        );
        apply_switch_setting(&mut s, "five", 100);
        apply_switch_setting(&mut s, "seven", 100);
        assert_eq!((s.auto_switch_five_hour, s.auto_switch_seven_day), (99, 100));
        apply_switch_setting(&mut s, "five", 10);
        apply_switch_setting(&mut s, "seven", 10);
        assert_eq!((s.auto_switch_five_hour, s.auto_switch_seven_day), (50, 50));
        apply_switch_setting(&mut s, "on", 0);
        apply_switch_setting(&mut s, "widget", 0);
        assert!(!s.auto_switch_enabled && !s.auto_switch_show_widget_toggle);
        apply_switch_setting(&mut s, "pace", 90);
        assert_eq!((s.critical_pace, s.critical_high_pace), (90, 90), "raising pace raises high pace");
        apply_switch_setting(&mut s, "high-pace", 60);
        assert_eq!(s.critical_high_pace, 90);
        apply_switch_setting(&mut s, "pace", 5);
        assert_eq!(s.critical_pace, 10);
        apply_switch_setting(&mut s, "quiet-start", 30);
        apply_switch_setting(&mut s, "quiet-end", -3);
        assert_eq!((s.critical_quiet_start, s.critical_quiet_end), (23, 0));
        apply_switch_setting(&mut s, "nonsense", 1);
        apply_critical_zone(&mut s, "Europe/Paris");
        assert_eq!(s.critical_time_zone, "Europe/Paris");
        apply_critical_zone(&mut s, "Mars/Base");
        assert_eq!(s.critical_time_zone, "Europe/Paris", "unknown zones are ignored");
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
