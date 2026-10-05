//! The refresh engine: fetch usage for every account, log it, build rows.

use std::collections::{HashMap, HashSet};

use claude_dashboard_core::api;
use claude_dashboard_core::extension_sources;
use claude_dashboard_core::rows::{build_rows, peak_utilization, BuildInput, DisplayRow};
use claude_dashboard_core::store::{self, UsageLogStore};
use claude_dashboard_core::usage::{UsageData, UsageLimit};

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshOutput {
    pub rows: Vec<DisplayRow>,
    pub peak: f64,
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

/// Account ids fed by a browser-extension install (empty if unreadable).
fn extension_account_ids() -> HashSet<String> {
    extension_sources::load(&extension_sources::sources_path())
        .map(|s| s.bindings.values().map(|b| b.account_id.clone()).collect())
        .unwrap_or_default()
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

    let ext = extension_account_ids();
    let rows = build_rows(BuildInput {
        accounts: &accounts,
        usage_by_account: &usage_by_account,
        errors: &errors,
        extension_install_account_ids: &ext,
        now_unix_s,
        active_claude_code_email: None,
    });
    let peak = peak_utilization(&rows);
    Ok(RefreshOutput { rows, peak })
}

/// How long the refresh loop waits between cycles: the user's Auto Refresh
/// setting, clamped/defaulted by `effective_refresh_seconds`.
pub fn loop_timeout(s: &claude_dashboard_core::settings::Settings) -> std::time::Duration {
    std::time::Duration::from_secs(s.effective_refresh_seconds())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeout_for(secs: u64) -> std::time::Duration {
        let s = claude_dashboard_core::settings::Settings { auto_refresh_seconds: secs, ..Default::default() };
        loop_timeout(&s)
    }

    #[test]
    fn loop_timeout_uses_effective_refresh_seconds() {
        use std::time::Duration;
        assert_eq!(timeout_for(0), Duration::from_secs(60));
        assert_eq!(timeout_for(5), Duration::from_secs(30));
        assert_eq!(timeout_for(99999), Duration::from_secs(3600));
        assert_eq!(timeout_for(120), Duration::from_secs(120));
    }
    use claude_dashboard_core::model::Account;

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
            extension_install_account_ids: &HashSet::new(),
            now_unix_s: NOW,
            active_claude_code_email: None,
        })
    }

    fn out(rows: Vec<DisplayRow>) -> RefreshOutput {
        let peak = peak_utilization(&rows);
        RefreshOutput { rows, peak }
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
