//! Display-row building and sort order for the GUI.
//!
//! Ports `apps/linux/lib/model.js` `buildRows` and `contract/README.md`
//! "Sort order": (1) pinned first; (2) active Claude Code account next, only
//! when nothing is pinned; (3) burn rate (5-hour utilization / time remaining)
//! descending, with sentinel -1 for non-active or no-usage accounts.

use std::collections::HashMap;

use crate::account_kind::kind_of;
use crate::extension_sources::ExtensionSources;
use crate::model::{Account, AccountPlan, AccountStatus};
use crate::usage::{UsageData, UsageLimit};

const FIVE_HOUR_SECONDS: f64 = 18000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowView {
    pub utilization: f64,
    pub resets_at_unix: Option<f64>,
    pub is_limited: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayRow {
    pub account_id: String,
    pub name: String,
    pub email: Option<String>,
    pub plan: AccountPlan,
    pub status: AccountStatus,
    pub five_hour: Option<WindowView>,
    pub seven_day: Option<WindowView>,
    pub fable: Option<WindowView>,
    /// Highest utilization across this row's windows (0 with no usage).
    pub peak_utilization: f64,
    /// The projection needs poll history (`BurnRateTracker`), which a pure
    /// row builder does not have; the caller fills this in.
    pub burn_projected_seconds: Option<f64>,
    /// Same projection for the 7-day and Fable windows (macOS shows an animal on each gauge).
    pub burn_projected_seconds_7d: Option<f64>,
    pub burn_projected_seconds_fable: Option<f64>,
    pub is_extension_sourced: bool,
    pub error: Option<String>,
    pub last_synced_unix: Option<f64>,
    pub is_active_claude_code: bool,
    pub is_pinned: bool,
    /// `AccountKind::settings_label()`: profile name, "Browser extension (X)" or "Pasted key".
    pub kind_label: String,
    /// `AccountKind::expired_hint()`, shown under "Session expired.".
    pub expired_hint: String,
}

pub struct BuildInput<'a> {
    pub accounts: &'a [Account],
    pub usage_by_account: &'a HashMap<String, UsageData>,
    pub errors: &'a HashMap<String, String>,
    pub sources: &'a ExtensionSources,
    pub now_unix_s: f64,
    pub active_claude_code_email: Option<&'a str>,
}

fn view(l: &UsageLimit) -> WindowView {
    WindowView {
        utilization: l.utilization,
        resets_at_unix: l.resets_at.map(|v| v as f64),
        is_limited: l.utilization >= 100.0,
    }
}

/// Port of `sortKey`: 5-hour utilization over time remaining. Read from the row, so it
/// also holds for a row spliced into an existing list; an errored row had no usage when
/// it was built, so it keys -1 whatever last-good windows it was backfilled with.
fn row_sort_key(row: &DisplayRow, now: f64) -> f64 {
    let Some(w) = row.five_hour.as_ref() else { return -1.0 };
    if row.status != AccountStatus::Active || row.error.is_some() {
        return -1.0;
    }
    let remaining = match w.resets_at_unix {
        None => FIVE_HOUR_SECONDS,
        Some(r) => (r - now).max(60.0),
    };
    w.utilization / remaining
}

/// The display order: 1 pinned, 2 active Claude Code (only when nothing is pinned),
/// 3 burn key. Stable.
pub fn sort_rows(rows: &mut [DisplayRow], now: f64) {
    let any_pinned = rows.iter().any(|r| r.is_pinned);
    let keys: HashMap<String, f64> =
        rows.iter().map(|r| (r.account_id.clone(), row_sort_key(r, now))).collect();
    rows.sort_by(|a, b| {
        b.is_pinned
            .cmp(&a.is_pinned)
            .then_with(|| {
                if any_pinned {
                    std::cmp::Ordering::Equal
                } else {
                    b.is_active_claude_code.cmp(&a.is_active_claude_code)
                }
            })
            .then(keys[&b.account_id].partial_cmp(&keys[&a.account_id]).unwrap_or(std::cmp::Ordering::Equal))
    });
}

pub fn build_rows(input: BuildInput<'_>) -> Vec<DisplayRow> {
    let mut rows: Vec<DisplayRow> = input
        .accounts
        .iter()
        .map(|a| {
            let usage = input.usage_by_account.get(&a.id);
            let five_hour = usage.map(|u| view(&u.five_hour));
            let seven_day = usage.map(|u| view(&u.seven_day));
            let fable = usage.and_then(|u| u.fable.as_ref()).map(view);
            let peak = [&five_hour, &seven_day, &fable]
                .into_iter()
                .flatten()
                .map(|w| w.utilization)
                .fold(0.0, f64::max);
            let kind = kind_of(a, input.sources);
            let row = DisplayRow {
                account_id: a.id.clone(),
                name: a.name.clone(),
                email: a.email.clone(),
                plan: a.plan.clone(),
                status: a.status.clone(),
                five_hour,
                seven_day,
                fable,
                peak_utilization: peak,
                burn_projected_seconds: None,
                burn_projected_seconds_7d: None,
                burn_projected_seconds_fable: None,
                is_extension_sourced: input.sources.bindings.values().any(|b| b.account_id == a.id),
                error: input.errors.get(&a.id).cloned(),
                last_synced_unix: a.last_synced_unix(),
                is_active_claude_code: input
                    .active_claude_code_email
                    .is_some_and(|e| a.email.as_deref() == Some(e)),
                is_pinned: a.is_pinned,
                kind_label: kind.settings_label(),
                expired_hint: kind.expired_hint(),
            };
            row
        })
        .collect();
    sort_rows(&mut rows, input.now_unix_s);
    rows
}

/// Port of `DashboardViewModel.menuBarPercentText`'s row choice: the pinned
/// row when it has usage, else the first row (in sort order) that has a 5-hour reading.
pub fn menu_bar_row(rows: &[DisplayRow]) -> Option<&DisplayRow> {
    rows.iter()
        .find(|r| r.is_pinned && r.five_hour.is_some())
        .or_else(|| rows.iter().find(|r| r.five_hour.is_some()))
}

/// The sidebar's per-account percent: `None` when expired; the 7-day figure
/// once it reaches 100, else the 5-hour figure.
pub fn sidebar_percent(row: &DisplayRow) -> Option<f64> {
    if row.status == AccountStatus::Expired {
        return None;
    }
    let five = row.five_hour.as_ref().map(|w| w.utilization);
    match row.seven_day.as_ref() {
        Some(w) if w.utilization >= 100.0 => Some(w.utilization),
        _ => five,
    }
}

pub fn peak_utilization(rows: &[DisplayRow]) -> f64 {
    rows.iter().map(|r| r.peak_utilization).fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_000_000.0;

    fn acct(id: &str, pinned: bool, status: AccountStatus) -> Account {
        let json = format!(
            r#"{{"id":"{id}","name":"{id}","chromeProfilePath":"","plan":"Pro","status":"{}","isPinned":{pinned}}}"#,
            match status {
                AccountStatus::Active => "active",
                AccountStatus::Expired => "expired",
                AccountStatus::Error => "error",
            }
        );
        Account::from_json_object(&json).unwrap()
    }

    fn usage(util: f64, resets_in: Option<i64>) -> UsageData {
        UsageData {
            five_hour: UsageLimit { utilization: util, resets_at: resets_in.map(|s| NOW as i64 + s) },
            seven_day: UsageLimit { utilization: 5.0, resets_at: None },
            fable: None,
        }
    }

    fn run(
        accounts: &[Account],
        usage: &[(&str, UsageData)],
        errors: &[(&str, &str)],
        ext: &[&str],
    ) -> Vec<DisplayRow> {
        let u: HashMap<String, UsageData> =
            usage.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let e: HashMap<String, String> =
            errors.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let mut x = ExtensionSources::default();
        for (i, id) in ext.iter().enumerate() {
            x.bind(&format!("inst-{i}"), id, "chrome");
        }
        build_rows(BuildInput {
            accounts,
            usage_by_account: &u,
            errors: &e,
            sources: &x,
            now_unix_s: NOW,
            active_claude_code_email: None,
        })
    }

    fn run_with_active(accounts: &[Account], usage: &[(&str, UsageData)], active: Option<&str>) -> Vec<DisplayRow> {
        let u: HashMap<String, UsageData> =
            usage.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        build_rows(BuildInput {
            accounts,
            usage_by_account: &u,
            errors: &HashMap::new(),
            sources: &ExtensionSources::default(),
            now_unix_s: NOW,
            active_claude_code_email: active,
        })
    }

    fn with_email(mut a: Account, email: &str) -> Account {
        a.email = Some(email.into());
        a
    }

    #[test]
    fn active_claude_code_account_sorts_after_pins_before_burn() {
        // b burns hardest, but c is the active Claude Code account and nothing is pinned.
        let accts = [
            with_email(acct("a", false, AccountStatus::Active), "a@x"),
            with_email(acct("b", false, AccountStatus::Active), "b@x"),
            with_email(acct("c", false, AccountStatus::Active), "c@x"),
        ];
        let usage = [("a", usage(10.0, Some(3600))), ("b", usage(90.0, Some(3600))), ("c", usage(5.0, Some(3600)))];
        let rows = run_with_active(&accts, &usage, Some("c@x"));
        assert_eq!(ids(&rows), vec!["c", "b", "a"]);
        assert!(rows[0].is_active_claude_code);
        assert!(!rows[1].is_active_claude_code);
    }

    #[test]
    fn any_pin_disables_the_claude_code_tier() {
        let accts = [
            with_email(acct("a", true, AccountStatus::Active), "a@x"),
            with_email(acct("b", false, AccountStatus::Active), "b@x"),
            with_email(acct("c", false, AccountStatus::Active), "c@x"),
        ];
        let usage = [("a", usage(1.0, Some(3600))), ("b", usage(90.0, Some(3600))), ("c", usage(5.0, Some(3600)))];
        let rows = run_with_active(&accts, &usage, Some("c@x"));
        assert_eq!(ids(&rows), vec!["a", "b", "c"]);
        assert!(rows[2].is_active_claude_code, "badge still set");
    }

    #[test]
    fn no_active_email_means_no_badge() {
        let accts = [with_email(acct("a", false, AccountStatus::Active), "a@x")];
        let rows = run_with_active(&accts, &[("a", usage(1.0, Some(3600)))], None);
        assert!(!rows[0].is_active_claude_code);
    }

    fn ids(rows: &[DisplayRow]) -> Vec<&str> {
        rows.iter().map(|r| r.account_id.as_str()).collect()
    }

    #[test]
    fn empty_input_gives_empty_rows() {
        assert!(run(&[], &[], &[], &[]).is_empty());
        assert_eq!(peak_utilization(&[]), 0.0);
    }

    #[test]
    fn orders_by_burn_rate_descending() {
        // a: 10/18000 (no reset); b: 50 over 3600s; c: 20 over 3600s.
        let accts = [
            acct("a", false, AccountStatus::Active),
            acct("b", false, AccountStatus::Active),
            acct("c", false, AccountStatus::Active),
        ];
        let rows = run(
            &accts,
            &[("a", usage(10.0, None)), ("b", usage(50.0, Some(3600))), ("c", usage(20.0, Some(3600)))],
            &[],
            &[],
        );
        assert_eq!(ids(&rows), ["b", "c", "a"]);
    }

    #[test]
    fn reset_time_floors_at_sixty_seconds() {
        // x: 10 / max(5,60) beats y: 20/3600.
        let accts = [acct("y", false, AccountStatus::Active), acct("x", false, AccountStatus::Active)];
        let rows = run(&accts, &[("y", usage(20.0, Some(3600))), ("x", usage(10.0, Some(5)))], &[], &[]);
        assert_eq!(ids(&rows), ["x", "y"]);
    }

    #[test]
    fn pinned_first_unconditionally() {
        let accts = [acct("hot", false, AccountStatus::Active), acct("pin", true, AccountStatus::Active)];
        let rows = run(&accts, &[("hot", usage(90.0, Some(3600))), ("pin", usage(1.0, None))], &[], &[]);
        assert_eq!(ids(&rows), ["pin", "hot"]);
    }

    #[test]
    fn non_active_and_no_usage_sink_to_bottom() {
        let accts = [
            acct("none", false, AccountStatus::Active),
            acct("exp", false, AccountStatus::Expired),
            acct("ok", false, AccountStatus::Active),
        ];
        let rows = run(&accts, &[("exp", usage(99.0, Some(3600))), ("ok", usage(1.0, None))], &[], &[]);
        assert_eq!(rows[0].account_id, "ok");
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn errored_account_keeps_row_with_error_and_sinks_when_no_usage() {
        let accts = [acct("bad", false, AccountStatus::Active), acct("ok", false, AccountStatus::Active)];
        let rows = run(&accts, &[("ok", usage(1.0, None))], &[("bad", "boom")], &[]);
        assert_eq!(ids(&rows), ["ok", "bad"]);
        assert_eq!(rows[1].error.as_deref(), Some("boom"));
        assert_eq!(rows[0].error, None);
    }

    #[test]
    fn extension_sourced_iff_in_set() {
        let accts = [acct("a", false, AccountStatus::Active), acct("b", false, AccountStatus::Active)];
        let rows = run(&accts, &[], &[], &["b"]);
        let a = rows.iter().find(|r| r.account_id == "a").unwrap();
        let b = rows.iter().find(|r| r.account_id == "b").unwrap();
        assert!(!a.is_extension_sourced);
        assert!(b.is_extension_sourced);
    }

    #[test]
    fn peak_is_max_across_rows_and_windows() {
        let accts = [acct("a", false, AccountStatus::Active), acct("b", false, AccountStatus::Active)];
        let mut ub = usage(30.0, None);
        ub.fable = Some(UsageLimit { utilization: 100.0, resets_at: None });
        let rows = run(&accts, &[("a", usage(40.0, None)), ("b", ub)], &[], &[]);
        assert_eq!(peak_utilization(&rows), 100.0);
        let b = rows.iter().find(|r| r.account_id == "b").unwrap();
        assert!(b.fable.as_ref().unwrap().is_limited);
        assert!(!b.five_hour.as_ref().unwrap().is_limited);
    }

    fn plain(id: &str, pinned: bool, status: AccountStatus, five: Option<f64>, seven: Option<f64>) -> DisplayRow {
        let a = acct(id, pinned, status);
        let mut rows = run(&[a], &[], &[], &[]);
        let mut r = rows.remove(0);
        let w = |u: f64| WindowView { utilization: u, resets_at_unix: None, is_limited: u >= 100.0 };
        r.five_hour = five.map(w);
        r.seven_day = seven.map(w);
        r
    }

    #[test]
    fn menu_bar_row_prefers_pinned() {
        let rows = vec![
            plain("a", false, AccountStatus::Active, Some(90.0), None),
            plain("b", true, AccountStatus::Active, Some(10.0), None),
        ];
        assert_eq!(menu_bar_row(&rows).unwrap().account_id, "b");
    }

    #[test]
    fn menu_bar_row_ignores_pinned_row_without_usage() {
        let rows = vec![
            plain("a", true, AccountStatus::Active, None, None),
            plain("b", false, AccountStatus::Active, Some(10.0), None),
        ];
        assert_eq!(menu_bar_row(&rows).unwrap().account_id, "b");
    }

    #[test]
    fn menu_bar_row_skips_rows_without_usage() {
        let rows = vec![
            plain("a", false, AccountStatus::Active, None, None),
            plain("b", false, AccountStatus::Active, Some(10.0), None),
        ];
        assert_eq!(menu_bar_row(&rows).unwrap().account_id, "b");
        assert!(menu_bar_row(&rows[..1]).is_none());
        assert!(menu_bar_row(&[]).is_none());
    }

    #[test]
    fn sidebar_percent_uses_7d_at_limit() {
        let r = plain("a", false, AccountStatus::Active, Some(40.0), Some(100.0));
        assert_eq!(sidebar_percent(&r), Some(100.0));
        let r = plain("a", false, AccountStatus::Active, Some(40.0), Some(99.0));
        assert_eq!(sidebar_percent(&r), Some(40.0));
    }

    #[test]
    fn sidebar_percent_none_when_expired() {
        let r = plain("a", false, AccountStatus::Expired, Some(40.0), Some(100.0));
        assert_eq!(sidebar_percent(&r), None);
    }

    #[test]
    fn row_carries_pin_and_kind_texts() {
        let rows = run(&[acct("a", true, AccountStatus::Active)], &[], &[], &[]);
        assert!(rows[0].is_pinned);
        assert!(!rows[0].expired_hint.is_empty());
    }
}
