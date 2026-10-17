//! Ping and Auto Ping (macOS `PingController` + `DashboardViewModel+Ping`). The rules here
//! are pure and clock-injected; one controller is shared by the refresh thread (auto
//! pings) and the UI (the account page's Ping button) through `CONTROLLER`.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use claude_dashboard_core::ping::PingOutcome;
use claude_dashboard_core::ping_log::{PingLogEntry, PingTrigger};
use claude_dashboard_core::usage::UsageData;

/// A failed auto ping may be retried after this long (macOS `failureCooldown`).
pub const FAILURE_COOLDOWN_S: f64 = 600.0;

/// In-memory ping state: the latch per "window not running" episode, the retry time after a
/// failed auto ping, the pings running now, and each account's latest logged ping.
#[derive(Default)]
pub struct PingController {
    latched: HashSet<String>,
    retry_after: HashMap<String, f64>,
    in_flight: HashSet<String>,
    latest: HashMap<String, PingLogEntry>,
}

pub static CONTROLLER: LazyLock<Mutex<PingController>> = LazyLock::new(Default::default);

/// Locks `CONTROLLER`, surviving a poisoned lock (a panicked ping thread).
pub fn controller() -> std::sync::MutexGuard<'static, PingController> {
    CONTROLLER.lock().unwrap_or_else(|e| e.into_inner())
}

/// A window is not running when the API reports no reset time for it.
pub fn window_not_running(u: &UsageData) -> bool {
    u.five_hour.resets_at.is_none() || u.seven_day.resets_at.is_none()
}

impl PingController {
    /// macOS `claimAutoPing`: true when account `id` should be auto-pinged now (and latches
    /// it). `usage` is this refresh's own successful fetch.
    pub fn claim_auto(&mut self, id: &str, active: bool, usage: &UsageData, enabled: bool, now: f64) -> bool {
        if !window_not_running(usage) {
            // Both windows run again: this episode is over.
            self.latched.remove(id);
            self.retry_after.remove(id);
            return false;
        }
        // A capped account cannot start a window; pinging it only fills the log.
        if usage.five_hour.utilization >= 100.0 || usage.seven_day.utilization >= 100.0 {
            return false;
        }
        if !enabled || !active || self.in_flight.contains(id) {
            return false;
        }
        if self.latched.contains(id) && !self.retry_after.get(id).is_some_and(|&until| now >= until) {
            return false;
        }
        self.latched.insert(id.to_string());
        self.retry_after.remove(id);
        true
    }

    /// Undoes a claim whose ping never ran (another ping for the account was running).
    pub fn release_claim(&mut self, id: &str) {
        self.latched.remove(id);
        self.retry_after.remove(id);
    }

    /// Marks a ping for `id` as running; false when one already is.
    pub fn begin(&mut self, id: &str) -> bool {
        self.in_flight.insert(id.to_string())
    }

    /// Records a finished ping: an ok ping latches the account; a failed auto ping may be
    /// retried after the cooldown.
    pub fn finish(&mut self, entry: PingLogEntry, now: f64) {
        let id = entry.account_id.clone();
        self.in_flight.remove(&id);
        if entry.outcome == PingOutcome::Ok {
            // A manual ping started the window too; do not auto-ping it again.
            self.latched.insert(id.clone());
            self.retry_after.remove(&id);
        } else if entry.trigger == PingTrigger::Auto {
            self.retry_after.insert(id.clone(), now + FAILURE_COOLDOWN_S);
        }
        self.latest.insert(id, entry);
    }

    pub fn is_in_flight(&self, id: &str) -> bool {
        self.in_flight.contains(id)
    }

    pub fn latest(&self, id: &str) -> Option<&PingLogEntry> {
        self.latest.get(id)
    }

    pub fn set_latest_all(&mut self, latest: HashMap<String, PingLogEntry>) {
        self.latest = latest;
    }

    /// The Ping Log was cleared: the account pages' captions go too.
    pub fn clear_latest(&mut self) {
        self.latest.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::usage::UsageLimit;

    const T: f64 = 1_000_000.0;

    fn usage(five: Option<i64>, seven: Option<i64>) -> UsageData {
        UsageData {
            five_hour: UsageLimit { utilization: 0.0, resets_at: five },
            seven_day: UsageLimit { utilization: 10.0, resets_at: seven },
            fable: None,
        }
    }

    fn running() -> UsageData {
        usage(Some(1), Some(2))
    }

    fn entry(id: &str, trigger: PingTrigger, outcome: PingOutcome) -> PingLogEntry {
        PingLogEntry {
            id: 1,
            account_id: id.into(),
            trigger,
            started_unix: T,
            finished_unix: T,
            outcome,
            detail: None,
        }
    }

    #[test]
    fn claims_when_5h_not_running() {
        assert!(PingController::default().claim_auto("a", true, &usage(None, Some(2)), true, T));
    }

    #[test]
    fn claims_when_7d_not_running() {
        assert!(PingController::default().claim_auto("a", true, &usage(Some(1), None), true, T));
    }

    #[test]
    fn no_claim_when_both_running() {
        assert!(!PingController::default().claim_auto("a", true, &running(), true, T));
    }

    #[test]
    fn no_claim_when_disabled() {
        assert!(!PingController::default().claim_auto("a", true, &usage(None, None), false, T));
    }

    #[test]
    fn no_claim_when_not_active() {
        assert!(!PingController::default().claim_auto("a", false, &usage(None, None), true, T));
    }

    #[test]
    fn no_claim_when_capped() {
        let mut u = usage(None, Some(2));
        u.five_hour.utilization = 100.0;
        assert!(!PingController::default().claim_auto("a", true, &u, true, T));
        let mut u = usage(None, Some(2));
        u.seven_day.utilization = 100.0;
        assert!(!PingController::default().claim_auto("a", true, &u, true, T));
    }

    #[test]
    fn latch_holds_until_both_windows_run() {
        let mut c = PingController::default();
        assert!(c.claim_auto("a", true, &usage(None, Some(2)), true, T));
        assert!(!c.claim_auto("a", true, &usage(None, Some(2)), true, T + 5.0), "still null right after");
        assert!(!c.claim_auto("a", true, &running(), true, T + 60.0));
        assert!(c.claim_auto("a", true, &usage(None, Some(2)), true, T + 18_000.0), "next reset");
    }

    #[test]
    fn running_window_clears_latch_even_when_disabled() {
        let mut c = PingController::default();
        assert!(c.claim_auto("a", true, &usage(None, None), true, T));
        assert!(!c.claim_auto("a", true, &running(), false, T + 1.0));
        assert!(c.claim_auto("a", true, &usage(None, None), true, T + 2.0));
    }

    #[test]
    fn failed_auto_retries_after_ten_minutes() {
        let mut c = PingController::default();
        let u = usage(None, Some(2));
        assert!(c.claim_auto("a", true, &u, true, T));
        assert!(c.begin("a"));
        c.finish(entry("a", PingTrigger::Auto, PingOutcome::Failed), T);
        assert!(!c.claim_auto("a", true, &u, true, T + 599.0));
        assert!(c.claim_auto("a", true, &u, true, T + 600.0));
    }

    #[test]
    fn ok_manual_ping_latches() {
        let mut c = PingController::default();
        assert!(c.begin("a"));
        c.finish(entry("a", PingTrigger::Manual, PingOutcome::Ok), T);
        assert!(!c.claim_auto("a", true, &usage(None, Some(2)), true, T + 1.0));
        assert_eq!(c.latest("a").map(|e| e.outcome), Some(PingOutcome::Ok));
    }

    #[test]
    fn in_flight_blocks_second_begin() {
        let mut c = PingController::default();
        assert!(c.begin("a"));
        assert!(c.is_in_flight("a"));
        assert!(!c.begin("a"));
        assert!(!c.claim_auto("a", true, &usage(None, None), true, T), "no auto ping while one runs");
        c.finish(entry("a", PingTrigger::Manual, PingOutcome::Failed), T);
        assert!(!c.is_in_flight("a"));
        assert!(c.begin("a"));
    }

    #[test]
    fn release_claim_allows_reclaim() {
        let mut c = PingController::default();
        let u = usage(None, Some(2));
        assert!(c.claim_auto("a", true, &u, true, T));
        c.release_claim("a");
        assert!(c.claim_auto("a", true, &u, true, T + 1.0));
    }

    #[test]
    fn window_not_running_reads_both_windows() {
        assert!(window_not_running(&usage(None, Some(1))));
        assert!(window_not_running(&usage(Some(1), None)));
        assert!(!window_not_running(&running()));
    }
}
