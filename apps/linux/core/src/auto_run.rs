//! Ports DashboardViewModel's reset monitor (`shouldRunSavedCommand` +
//! `pingedAccounts`, DashboardViewModel.swift:208-211, 278-293). The signal is
//! "a window has no reset time" — the API drops `resets_at` between one cycle
//! ending and the next starting. The latch fires once per such episode.

use std::collections::HashSet;

use crate::rows::{DisplayRow, WindowView};

/// `shouldRunSavedCommand(for:)`: usage present and the 5h or 7d window has no reset time.
pub fn should_run_saved_command(row: &DisplayRow) -> bool {
    let missing = |w: &Option<WindowView>| matches!(w, Some(v) if v.resets_at_unix.is_none());
    missing(&row.five_hour) || missing(&row.seven_day)
}

#[derive(Debug, Default)]
pub struct AutoRunLatch {
    pinged: HashSet<String>,
}

impl AutoRunLatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ids whose saved command should run on this pass.
    pub fn due(&mut self, rows: &[DisplayRow], has_command: impl Fn(&str) -> bool) -> Vec<String> {
        let mut out = Vec::new();
        for row in rows {
            let id = row.account_id.as_str();
            if should_run_saved_command(row) {
                if !self.pinged.contains(id) && has_command(id) {
                    self.pinged.insert(id.to_string());
                    out.push(id.to_string());
                }
            } else {
                self.pinged.remove(id);
            }
        }
        let live: HashSet<&str> = rows.iter().map(|r| r.account_id.as_str()).collect();
        self.pinged.retain(|id| live.contains(id.as_str()));
        out
    }

    pub fn armed_count(&self) -> usize {
        self.pinged.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountPlan, AccountStatus};

    fn w(resets: Option<f64>) -> Option<WindowView> {
        Some(WindowView { utilization: 10.0, resets_at_unix: resets, is_limited: false })
    }

    fn row(id: &str, five: Option<WindowView>, seven: Option<WindowView>) -> DisplayRow {
        DisplayRow {
            account_id: id.into(),
            name: id.into(),
            email: None,
            plan: AccountPlan::Pro,
            status: AccountStatus::Active,
            five_hour: five,
            seven_day: seven,
            fable: None,
            peak_utilization: 10.0,
            burn_projected_seconds: None,
            is_extension_sourced: false,
            error: None,
            last_synced_unix: None,
            is_active_claude_code: false,
        }
    }

    fn reset(id: &str) -> DisplayRow { row(id, w(None), w(Some(5.0))) }
    fn ticking(id: &str) -> DisplayRow { row(id, w(Some(1.0)), w(Some(5.0))) }
    fn all(_: &str) -> bool { true }

    #[test]
    fn rule_needs_usage_and_a_missing_reset() {
        assert!(should_run_saved_command(&reset("a")));
        assert!(should_run_saved_command(&row("a", w(Some(1.0)), w(None))));
        assert!(!should_run_saved_command(&ticking("a")));
        assert!(!should_run_saved_command(&row("a", None, None)), "no usage -> never");
    }

    #[test]
    fn fires_once_per_episode() {
        let mut l = AutoRunLatch::new();
        assert_eq!(l.due(&[reset("a")], all), vec!["a".to_string()]);
        assert!(l.due(&[reset("a")], all).is_empty());
        assert!(l.due(&[reset("a")], all).is_empty());
    }

    #[test]
    fn rearms_only_when_both_windows_tick_again() {
        let mut l = AutoRunLatch::new();
        l.due(&[reset("a")], all);
        assert!(l.due(&[ticking("a")], all).is_empty());
        assert_eq!(l.due(&[reset("a")], all), vec!["a".to_string()]);
    }

    #[test]
    fn never_fires_without_a_saved_command() {
        let mut l = AutoRunLatch::new();
        assert!(l.due(&[reset("a")], |_| false).is_empty());
        assert_eq!(l.armed_count(), 0, "does not arm");
    }

    #[test]
    fn command_saved_mid_episode_fires_next_pass() {
        let mut l = AutoRunLatch::new();
        assert!(l.due(&[reset("a")], |_| false).is_empty());
        assert_eq!(l.due(&[reset("a")], all), vec!["a".to_string()]);
    }

    #[test]
    fn accounts_are_independent() {
        let mut l = AutoRunLatch::new();
        let due = l.due(&[reset("a"), ticking("b"), reset("c")], |id| id != "c");
        assert_eq!(due, vec!["a".to_string()]);
    }

    #[test]
    fn vanished_account_releases_the_latch() {
        let mut l = AutoRunLatch::new();
        l.due(&[reset("a")], all);
        assert_eq!(l.armed_count(), 1);
        l.due(&[ticking("b")], all);
        assert_eq!(l.armed_count(), 0);
    }
}
