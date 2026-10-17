//! The refresh engine: fetch usage for every account, log it, build rows.

use std::collections::{HashMap, HashSet};

use claude_dashboard_core::api;
use claude_dashboard_core::burn_rate::BurnRateTracker;
use claude_dashboard_core::model::{Account, AccountStatus};
use claude_dashboard_core::extension_sources;
use claude_dashboard_core::rows::{build_rows, peak_utilization, BuildInput, DisplayRow, WindowView, sort_rows};
use claude_dashboard_core::store::{self, UsageLogStore};
use claude_dashboard_core::usage::{UsageData, UsageLimit};

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshOutput {
    pub rows: Vec<DisplayRow>,
    pub peak: f64,
}

/// What wakes the refresh loop: every account, just one (the account page's Refresh), or
/// `Reschedule`: the Auto Refresh setting changed, so restart the wait with the new
/// interval without fetching anything (macOS only reschedules its timer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nudge {
    All,
    One(String),
    Reschedule,
}

/// Folds the nudges that piled up while a cycle ran: any `All` covers everything, otherwise
/// each distinct account once, in arrival order. `Reschedule` is never a cycle: it is
/// dropped, so a batch of only `Reschedule`s yields nothing to run and the loop simply
/// waits again with a freshly read interval.
pub fn coalesce(nudges: Vec<Nudge>) -> Vec<Nudge> {
    if nudges.contains(&Nudge::All) {
        return vec![Nudge::All];
    }
    let mut out: Vec<Nudge> = Vec::new();
    for n in nudges {
        if n != Nudge::Reschedule && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// Puts `fresh` in place of the row with the same account id (appended when `prev` has none)
/// and re-sorts with the rule `build_rows` uses. Every other row is untouched, projections
/// included.
pub fn merge_one(prev: &[DisplayRow], fresh: DisplayRow, now_unix_s: f64) -> Vec<DisplayRow> {
    let mut rows = prev.to_vec();
    match rows.iter_mut().find(|r| r.account_id == fresh.account_id) {
        Some(slot) => *slot = fresh,
        None => rows.push(fresh),
    }
    sort_rows(&mut rows, now_unix_s);
    rows
}

/// Refreshes one account and keeps every other row from `prev` as it was (their burn
/// projections included). A failure to read the store, or an account that no longer
/// exists, leaves `prev` unchanged.
pub fn refresh_one(account_id: &str, prev: &[DisplayRow], now_unix_s: f64) -> RefreshOutput {
    let fresh = store::load_accounts()
        .map_err(|e| e.to_string())
        .and_then(|all| {
            let one: Vec<Account> = all.into_iter().filter(|a| a.id == account_id).collect();
            if one.is_empty() { Err(format!("account {account_id} not found")) } else { Ok(one) }
        })
        .map(|one| fetch_and_build(&one, now_unix_s));
    match fresh {
        Ok(mut rows) if rows.len() == 1 => {
            backfill_errored_rows(prev, &mut rows);
            let merged = merge_one(prev, rows.remove(0), now_unix_s);
            let peak = peak_utilization(&merged);
            RefreshOutput { rows: merged, peak }
        }
        Ok(_) => merge_errors(prev, Err("unexpected row count".into())),
        Err(e) => merge_errors(prev, Err(e)),
    }
}

/// Ok -> the fresh output; Err -> the previous rows, unchanged.
pub fn merge_errors(prev: &[DisplayRow], fresh: Result<RefreshOutput, String>) -> RefreshOutput {
    match fresh {
        Ok(mut out) => {
            backfill_errored_rows(prev, &mut out.rows);
            out.peak = peak_utilization(&out.rows);
            out
        }
        Err(e) => {
            eprintln!("refresh failed, keeping previous rows: {e}");
            RefreshOutput { rows: prev.to_vec(), peak: peak_utilization(prev) }
        }
    }
}

/// Feeds each healthy row's windows to the tracker and stores the projected
/// seconds-to-limit on the row, as macOS does after each fetch
/// (`DashboardViewModel.refresh`: 5h and 7d always, Fable when present; a
/// missing reset time falls back to now + the window length). Expired or
/// errored rows get no projection.
pub fn apply_projections(tracker: &mut BurnRateTracker, rows: &mut [DisplayRow], now_unix_s: f64) {
    for row in rows.iter_mut() {
        row.burn_projected_seconds = None;
        row.burn_projected_seconds_7d = None;
        row.burn_projected_seconds_fable = None;
        if row.error.is_some() || row.status != AccountStatus::Active {
            continue;
        }
        let mut project = |w: &Option<WindowView>, window: i64, total_s: f64| {
            let w = w.as_ref()?;
            let resets = w.resets_at_unix.unwrap_or(now_unix_s + total_s);
            tracker
                .record(&row.account_id, window, w.utilization, Some(resets), now_unix_s)
                .map(|r| r.projected_time)
        };
        row.burn_projected_seconds = project(&row.five_hour, WINDOW_FIVE_HOUR, 18000.0);
        row.burn_projected_seconds_7d = project(&row.seven_day, WINDOW_SEVEN_DAY, 604800.0);
        row.burn_projected_seconds_fable = project(&row.fable, WINDOW_FABLE, 604800.0);
    }
}

/// An errored row with no window data of its own keeps the previous row's last-good
/// windows, so a transient failure does not blank its gauges.
fn backfill_errored_rows(prev: &[DisplayRow], fresh: &mut [DisplayRow]) {
    for row in fresh.iter_mut() {
        if row.error.is_none() || row.five_hour.is_some() || row.seven_day.is_some() || row.fable.is_some() {
            continue;
        }
        if let Some(p) = prev.iter().find(|p| p.account_id == row.account_id) {
            row.five_hour = p.five_hour.clone();
            row.seven_day = p.seven_day.clone();
            row.fable = p.fable.clone();
            row.peak_utilization = p.peak_utilization;
            if row.last_synced_unix.is_none() {
                row.last_synced_unix = p.last_synced_unix;
            }
        }
    }
}

const WINDOW_FIVE_HOUR: i64 = 0;
const WINDOW_SEVEN_DAY: i64 = 1;
const WINDOW_FABLE: i64 = 3;

type Fetched = (UsageData, Option<String>);

fn fetch_one(org_id: &str, session_key: &str) -> Result<Fetched, String> {
    let resp = api::usage_raw(org_id, session_key).map_err(|e| e.to_string())?;
    if resp.body.is_empty() {
        return Err("Empty response.".into());
    }
    let usage = UsageData::decode(&resp.body).map_err(|e| e.to_string())?;
    Ok((usage, resp.new_session_key))
}

/// The extension install bindings (empty if unreadable).
fn load_sources() -> extension_sources::ExtensionSources {
    extension_sources::load(&extension_sources::sources_path()).unwrap_or_default()
}

/// Saves rotated session keys and last_synced stamps in one locked load-modify-save (shared with the
/// bridge writers), after all fetches so there are no write races.
/// Reference-date (2001-01-01) offset from the Unix epoch, as `Account.last_synced` uses.
const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0;

fn persist_refresh(rotated: &HashMap<String, String>, synced: &HashSet<String>, now_unix_s: f64) {
    if rotated.is_empty() && synced.is_empty() {
        return;
    }
    let Ok(_lock) = store::lock_store() else { return };
    let Ok((mut accounts, _)) = store::load_accounts_for_write() else { return };
    for a in accounts.iter_mut() {
        if let Some(k) = rotated.get(&a.id) {
            a.session_key = Some(store::encrypt_session_key(k));
        }
        if synced.contains(&a.id) {
            a.last_synced = Some(now_unix_s - REFERENCE_EPOCH_OFFSET);
        }
    }
    if let Err(e) = store::save_accounts(&accounts) {
        eprintln!("could not persist refresh results: {e}");
    }
}

/// Loads accounts, fetches usage per account (one thread each), logs, builds
/// rows. A store failure yields no rows; the caller merges via `merge_errors`.
pub fn refresh_once(now_unix_s: f64) -> Result<RefreshOutput, String> {
    let accounts = match store::load_accounts() {
        Ok(a) => a,
        Err(e) => return Err(e.to_string()),
    };
    let rows = fetch_and_build(&accounts, now_unix_s);
    let peak = peak_utilization(&rows);
    Ok(RefreshOutput { rows, peak })
}

/// Fetches usage for `accounts` (one thread each), saves rotated keys and sync stamps,
/// logs the readings and builds their sorted rows.
fn fetch_and_build(accounts: &[Account], now_unix_s: f64) -> Vec<DisplayRow> {
    let results: Vec<(String, Result<Fetched, String>)> = std::thread::scope(|s| {
        let handles: Vec<_> = accounts
            .iter()
            .map(|a| {
                s.spawn(move || {
                    let key = a.session_key.as_deref().and_then(store::decrypt_session_key);
                    let r = match (&a.org_id, key) {
                        (Some(org), Some(key)) => fetch_one(org, &key),
                        _ => Err("Not configured".to_string()),
                    };
                    (a.id.clone(), r)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| (String::new(), Err("worker panicked".into()))))
            .collect()
    });

    let mut usage_by_account: HashMap<String, UsageData> = HashMap::new();
    let mut errors: HashMap<String, String> = HashMap::new();
    let mut rotated: HashMap<String, String> = HashMap::new();
    for (id, r) in results {
        match r {
            Ok((u, new_key)) => {
                if let Some(k) = new_key {
                    rotated.insert(id.clone(), k);
                }
                usage_by_account.insert(id, u);
            }
            Err(e) => {
                errors.insert(id, e);
            }
        }
    }

    let synced: HashSet<String> = usage_by_account.keys().cloned().collect();
    persist_refresh(&rotated, &synced, now_unix_s);

    if let Ok(mut log) = UsageLogStore::open() {
        for (id, u) in &usage_by_account {
            let mut rec = |w: i64, l: &UsageLimit| {
                log.record(id, w, l.resets_at.unwrap_or(0) as f64, l.utilization, l.utilization >= 100.0);
            };
            rec(WINDOW_FIVE_HOUR, &u.five_hour);
            rec(WINDOW_SEVEN_DAY, &u.seven_day);
            if let Some(f) = &u.fable {
                rec(WINDOW_FABLE, f);
            }
        }
    }

    let sources = load_sources();
    let active_email = claude_dashboard_core::claude_code::active_email();
    build_rows(BuildInput {
        accounts,
        usage_by_account: &usage_by_account,
        errors: &errors,
        sources: &sources,
        now_unix_s,
        active_claude_code_email: active_email.as_deref(),
    })
}

/// How long the refresh loop waits between cycles: the user's Auto Refresh
/// setting in minutes (clamped), or `None` when disabled, meaning wait only
/// for a nudge.
pub fn loop_timeout(s: &claude_dashboard_core::settings::Settings) -> Option<std::time::Duration> {
    s.auto_refresh_enabled
        .then(|| std::time::Duration::from_secs(s.effective_refresh_minutes() * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeout_for(enabled: bool, minutes: u64) -> Option<std::time::Duration> {
        let s = claude_dashboard_core::settings::Settings {
            auto_refresh_enabled: enabled,
            auto_refresh_minutes: minutes,
            ..Default::default()
        };
        loop_timeout(&s)
    }

    #[test]
    fn loop_timeout_uses_minutes_and_none_when_disabled() {
        use std::time::Duration;
        assert_eq!(timeout_for(false, 5), None);
        assert_eq!(timeout_for(true, 5), Some(Duration::from_secs(300)));
        assert_eq!(timeout_for(true, 0), Some(Duration::from_secs(60)));
        assert_eq!(timeout_for(true, 999), Some(Duration::from_secs(3600)));
    }

    #[test]
    fn projections_follow_rising_usage_per_window_and_skip_bad_rows() {
        let mut t = BurnRateTracker::default();
        let reset = Some(NOW + 3600.0);
        let w = |u: f64| Some(WindowView { utilization: u, resets_at_unix: reset, is_limited: false });
        let mk = |u5: f64, u7: f64| {
            let mut rs = rows(&[("c", "boom")]);
            for r in rs.iter_mut() {
                r.five_hour = w(u5);
                r.seven_day = w(u7);
                r.fable = None;
            }
            rs
        };
        let mut first = mk(10.0, 5.0);
        apply_projections(&mut t, &mut first, NOW);
        assert!(first.iter().all(|r| r.burn_projected_seconds.is_none()), "first record has no rate");

        // 5h rises 10 -> 20 in 600 s (rate 1/60 %/s -> 4800 s left); 7d unchanged.
        let mut second = mk(20.0, 5.0);
        apply_projections(&mut t, &mut second, NOW + 600.0);
        let a = second.iter().find(|r| r.account_id == "a").unwrap();
        assert!((a.burn_projected_seconds.unwrap() - 4800.0).abs() < 1e-6);
        assert!(a.burn_projected_seconds_7d.is_none(), "unchanged usage projects nothing");
        assert!(a.burn_projected_seconds_fable.is_none());
        let c = second.iter().find(|r| r.account_id == "c").unwrap();
        assert!(c.error.is_some() && c.burn_projected_seconds.is_none(), "errored row gets none");
    }

    const NOW: f64 = 1_000_000.0;

    fn acct(id: &str) -> Account {
        Account::from_json_object(&format!(
            r#"{{"id":"{id}","name":"{id}","chromeProfilePath":"","plan":"Pro","status":"active"}}"#
        ))
        .unwrap()
    }

    fn usage(util: f64) -> UsageData {
        UsageData {
            five_hour: UsageLimit { utilization: util, resets_at: None },
            seven_day: UsageLimit { utilization: 5.0, resets_at: None },
            fable: None,
        }
    }

    fn rows(errors: &[(&str, &str)]) -> Vec<DisplayRow> {
        let accts = [acct("a"), acct("b"), acct("c")];
        let u: HashMap<String, UsageData> = [("a", 40.0), ("b", 70.0), ("c", 10.0)]
            .iter()
            .map(|(k, v)| (k.to_string(), usage(*v)))
            .collect();
        let e: HashMap<String, String> =
            errors.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        build_rows(BuildInput {
            accounts: &accts,
            usage_by_account: &u,
            errors: &e,
            sources: &extension_sources::ExtensionSources::default(),
            now_unix_s: NOW,
            active_claude_code_email: None,
        })
    }

    fn out(rows: Vec<DisplayRow>) -> RefreshOutput {
        let peak = peak_utilization(&rows);
        RefreshOutput { rows, peak }
    }

    #[test]
    fn refresh_one_keeps_other_rows() {
        let mut prev = rows(&[]);
        assert_eq!(prev.iter().map(|r| r.account_id.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        for r in prev.iter_mut() {
            r.burn_projected_seconds = Some(1234.0);
        }
        // b was 70% and led; it drops to 5%, so the order becomes a, c, b.
        let mut fresh = prev.iter().find(|r| r.account_id == "b").unwrap().clone();
        fresh.five_hour = Some(WindowView { utilization: 5.0, resets_at_unix: None, is_limited: false });
        fresh.burn_projected_seconds = Some(99.0);
        let merged = merge_one(&prev, fresh.clone(), NOW);
        assert_eq!(merged.iter().map(|r| r.account_id.as_str()).collect::<Vec<_>>(), ["a", "c", "b"]);
        for id in ["a", "c"] {
            let old = prev.iter().find(|r| r.account_id == id).unwrap();
            let new = merged.iter().find(|r| r.account_id == id).unwrap();
            assert_eq!(new, old, "{id} untouched, projection kept");
        }
        assert_eq!(merged.iter().find(|r| r.account_id == "b").unwrap(), &fresh);
    }

    #[test]
    fn merge_one_appends_unknown_and_pin_stays_first() {
        let mut prev = rows(&[]);
        prev.iter_mut().find(|r| r.account_id == "c").unwrap().is_pinned = true;
        prev.sort_by_key(|r| r.account_id != "c");
        let mut fresh = prev.iter().find(|r| r.account_id == "b").unwrap().clone();
        fresh.five_hour = Some(WindowView { utilization: 99.0, resets_at_unix: None, is_limited: false });
        let merged = merge_one(&prev, fresh, NOW);
        assert_eq!(merged[0].account_id, "c", "pinned row leads");
        assert_eq!(merged[1].account_id, "b");
        let mut extra = merged[2].clone();
        extra.account_id = "z".into();
        assert_eq!(merge_one(&merged, extra, NOW).len(), 4);
    }

    #[test]
    fn coalesce_folds_pending_nudges() {
        use Nudge::*;
        assert_eq!(coalesce(vec![One("a".into()), All, One("b".into())]), vec![All]);
        assert_eq!(
            coalesce(vec![One("a".into()), One("b".into()), One("a".into())]),
            vec![One("a".into()), One("b".into())]
        );
        assert!(coalesce(vec![]).is_empty());
    }

    #[test]
    fn reschedule_never_becomes_a_refresh() {
        use Nudge::*;
        assert!(coalesce(vec![Reschedule]).is_empty(), "only re-wait");
        assert!(coalesce(vec![Reschedule, Reschedule]).is_empty());
        assert_eq!(coalesce(vec![Reschedule, All, Reschedule]), vec![All], "All wins");
        assert_eq!(
            coalesce(vec![Reschedule, One("a".into()), Reschedule]),
            vec![One("a".into())],
            "One wins"
        );
    }

    #[test]
    fn merge_errors_keeps_previous_rows_on_failure() {
        let prev = rows(&[]);
        let merged = merge_errors(&prev, Err("offline".into()));
        assert_eq!(merged.rows, prev);
        assert_eq!(merged.peak, 70.0);
    }

    #[test]
    fn whole_refresh_failure_keeps_last_good_rows() {
        let prev = rows(&[("a", "old")]);
        let merged = merge_errors(&prev, Err("store unreadable".into()));
        assert_eq!(merged.rows.len(), 3);
        assert_eq!(merged.rows, prev);
    }

    #[test]
    fn errored_row_keeps_previous_window_data_not_blank() {
        let prev = rows(&[]);
        let mut fresh = rows(&[("b", "HTTP 500")]);
        let b = fresh.iter_mut().find(|r| r.account_id == "b").unwrap();
        b.five_hour = None;
        b.seven_day = None;
        b.fable = None;
        b.peak_utilization = 0.0;
        let merged = merge_errors(&prev, Ok(out(fresh)));
        let b = merged.rows.iter().find(|r| r.account_id == "b").unwrap();
        assert_eq!(b.error.as_deref(), Some("HTTP 500"));
        assert_eq!(b.five_hour.as_ref().map(|w| w.utilization), Some(70.0));
        assert!(b.seven_day.is_some());
        assert_eq!(merged.peak, 70.0);
        assert_eq!(merged.rows.len(), 3);
    }

    #[test]
    fn merge_errors_takes_fresh_on_success() {
        let prev = rows(&[]);
        let fresh = out(vec![prev[0].clone()]);
        let merged = merge_errors(&prev, Ok(fresh.clone()));
        assert_eq!(merged, fresh);
    }

    #[test]
    fn refresh_merges_errors_without_dropping_rows() {
        let merged = merge_errors(&[], Ok(out(rows(&[("a", "HTTP 500")]))));
        assert_eq!(merged.rows.len(), 3);
        let a = merged.rows.iter().find(|r| r.account_id == "a").unwrap();
        assert_eq!(a.error.as_deref(), Some("HTTP 500"));
        assert_eq!(a.five_hour.as_ref().map(|w| w.utilization), Some(40.0));
        assert_eq!(merged.rows.iter().filter(|r| r.error.is_none()).count(), 2);
    }
}
