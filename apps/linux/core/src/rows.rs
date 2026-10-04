//! Display-row building and sort order for the GUI.
//!
//! Ports `apps/linux/lib/model.js` `buildRows` and `contract/README.md`
//! "Sort order": (1) pinned first; (2) active Claude Code account next, only
//! when nothing is pinned -- that signal is absent in this sub-project, so the
//! tier never fires; (3) burn rate (5-hour utilization / time remaining)
//! descending, with sentinel -1 for non-active or no-usage accounts.

use std::collections::{HashMap, HashSet};

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
    pub is_extension_sourced: bool,
    pub error: Option<String>,
    pub last_synced_unix: Option<f64>,
}

pub struct BuildInput<'a> {
    pub accounts: &'a [Account],
    pub usage_by_account: &'a HashMap<String, UsageData>,
    pub errors: &'a HashMap<String, String>,
    pub extension_install_account_ids: &'a HashSet<String>,
    pub now_unix_s: f64,
}

fn view(l: &UsageLimit) -> WindowView {
    WindowView {
        utilization: l.utilization,
        resets_at_unix: l.resets_at.map(|v| v as f64),
        is_limited: l.utilization >= 100.0,
    }
}

/// Port of `sortKey`: 5-hour utilization over time remaining.
fn sort_key(usage: Option<&UsageData>, status: &AccountStatus, now: f64) -> f64 {
    let Some(u) = usage else { return -1.0 };
    if *status != AccountStatus::Active {
        return -1.0;
    }
    let remaining = match u.five_hour.resets_at {
        None => FIVE_HOUR_SECONDS,
        Some(r) => (r as f64 - now).max(60.0),
    };
    u.five_hour.utilization / remaining
}

pub fn build_rows(input: BuildInput<'_>) -> Vec<DisplayRow> {
    let mut keyed: Vec<(bool, f64, DisplayRow)> = input
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
            let key = sort_key(usage, &a.status, input.now_unix_s);
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
                is_extension_sourced: input.extension_install_account_ids.contains(&a.id),
                error: input.errors.get(&a.id).cloned(),
                last_synced_unix: a.last_synced_unix(),
            };
            (a.is_pinned, key, row)
        })
        .collect();
    // Tier 2 (active Claude Code) is absent here. Stable sort, like JS.
    keyed.sort_by(|a, b| {
        b.0.cmp(&a.0).then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    keyed.into_iter().map(|(_, _, r)| r).collect()
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
        let x: HashSet<String> = ext.iter().map(|s| s.to_string()).collect();
        build_rows(BuildInput {
            accounts,
            usage_by_account: &u,
            errors: &e,
            extension_install_account_ids: &x,
            now_unix_s: NOW,
        })
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
}
