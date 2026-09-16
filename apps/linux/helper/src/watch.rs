//! `claude-dashboard-helper watch` — the polling daemon.
//!
//! Linux-only; deliberately absent from the usage banner, which is byte-exact
//! shared contract (see `main.rs` and `contract/helper-cli.md`, "Linux-only
//! commands").
//!
//! It performs I/O and records observations. It does not compute burn rate,
//! sort accounts, or choose the panel row: those rules live in
//! `apps/linux/lib/` and are pinned by `contract/`, and a second
//! implementation of them here would be a third copy of rules that already
//! exist in JS and Swift.

use crate::config;
use crate::state;
use crate::usage_log;
use claude_dashboard_core::api::{usage_raw, ApiError};
use claude_dashboard_core::model::{Account, AccountStatus};
use claude_dashboard_core::store;
use claude_dashboard_core::usage::UsageData;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct PollEnv<'a> {
    /// The account store, as loaded for this pass. `Err` means the store
    /// exists but could not be read or parsed — a poll that never happened —
    /// and must not be confused with `Ok(vec![])`, a store that was read
    /// fine and genuinely holds no accounts. See
    /// `contract/linux-state.md`'s "`fatal`" section.
    pub accounts: Result<Vec<Account>, String>,
    pub fetch: &'a dyn Fn(&str, &str) -> Result<String, String>,
    pub now_ms: i64,
    pub interval_seconds: u64,
    pub active_email: Option<String>,
}

/// One poll pass, with every side effect injected so it is testable without a
/// browser, a keychain, a network or a clock.
pub fn poll_once(env: &PollEnv) -> Value {
    let accounts: &[Account] = match &env.accounts {
        Ok(accounts) => accounts,
        // An unreadable store is reported as `accounts: []` too — there is
        // nothing else to project — but `fatal` below tells the reader this
        // is not the same thing as "read fine, zero accounts".
        Err(_) => &[],
    };

    let mut usage: BTreeMap<String, Value> = BTreeMap::new();
    let mut errors: BTreeMap<String, String> = BTreeMap::new();

    for account in accounts {
        // Same inclusion filter as `decrypt`: active, and an orgId to fetch
        // against. An excluded account still appears in `accounts` — the UI
        // exists partly to repair those.
        if account.status != AccountStatus::Active {
            continue;
        }
        let (Some(org_id), Some(cipher)) = (&account.org_id, &account.session_key) else {
            continue;
        };
        let key = store::decrypt_session_key(cipher).unwrap_or_else(|| cipher.clone());

        match (env.fetch)(org_id, &key) {
            Ok(body) => match serde_json::from_str::<Value>(&body) {
                Ok(payload) => {
                    usage.insert(account.id.clone(), payload);
                }
                Err(e) => {
                    errors.insert(account.id.clone(), format!("Malformed usage payload: {e}"));
                }
            },
            Err(message) => {
                errors.insert(account.id.clone(), message);
            }
        }
    }

    // `contract/account-schema.md`'s "An unreadable store is not an empty
    // store": the third outcome (unreadable) must never reach the rest of
    // the program disguised as the second (empty). Two distinct fatal codes
    // say which happened.
    let fatal = match &env.accounts {
        Err(_) => Some("store-unreadable"),
        Ok(accounts) if accounts.is_empty() => Some("no-accounts"),
        Ok(_) => None,
    };

    state::build_state(
        accounts,
        &usage,
        &errors,
        env.active_email.as_deref(),
        env.now_ms,
        env.interval_seconds,
        fatal,
    )
}

/// Appends one row per window for every account whose payload decoded.
///
/// This is the one place the daemon decodes anything, and it does so through
/// `core::usage::UsageData::decode` — which already exists and is already
/// verified against `contract/cases/usage-decoding.json`, the same fixture the
/// JS decoder runs. So no new implementation of the Fable rule is created
/// here. What still holds is that the daemon *publishes* nothing decoded:
/// `state.json` carries the payload verbatim, and consumers decode it
/// themselves.
pub fn record_usage(log: &mut usage_log::UsageLog, snapshot: &Value, now_ms: i64) {
    let Some(usage) = snapshot["usage"].as_object() else {
        return;
    };
    for (account_id, payload) in usage {
        let text = payload.to_string();
        // A malformed or incomplete payload is skipped whole: half a reading
        // is worse than none, because the charts would read the gap as real.
        let Ok(data) = UsageData::decode(&text) else {
            continue;
        };

        // Matches macOS's `BurnRateTracker.swift:33`: `isLimited = utilization
        // >= 100.0`. `utilization` here is already the 0-100 percent scale
        // (see `encode_utilization`'s `* 100` for the *basis-points* `u`
        // column, a separate encoding) — not a 0-1 fraction — so no rescale
        // is needed before the comparison.
        let mut write = |w: i64, utilization: f64, resets_at: Option<i64>| {
            log.record(
                account_id,
                w,
                resets_at.unwrap_or(0) * 1000,
                utilization,
                utilization >= 100.0,
                now_ms,
            );
        };
        write(usage_log::WINDOW_FIVE_HOUR, data.five_hour.utilization, data.five_hour.resets_at);
        write(usage_log::WINDOW_SEVEN_DAY, data.seven_day.utilization, data.seven_day.resets_at);
        if let Some(fable) = data.fable {
            write(usage_log::WINDOW_FABLE, fable.utilization, fable.resets_at);
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn fetch_live(org_id: &str, session_key: &str) -> Result<String, String> {
    match usage_raw(org_id, session_key) {
        Ok(resp) if resp.body.is_empty() => Err("Empty response.".to_string()),
        Ok(resp) => Ok(resp.body),
        Err(ApiError::InvalidOrgId) => Err("Invalid orgId.".to_string()),
        Err(ApiError::HttpError(status)) => Err(format!("HTTP {status}")),
        Err(ApiError::Timeout) => Err("Request timed out.".to_string()),
        Err(ApiError::Network(desc)) => Err(format!("Network error: {desc}")),
    }
}

/// Applies one pass' worth of readings to `usage-log.json` on disk, reading
/// it, parsing it and writing it back — or refusing to, when refusing is the
/// safe choice.
///
/// Three outcomes, matching `contract/account-schema.md`'s "An unreadable
/// store is not an empty store" (the same rule, applied to this file):
/// * the log does not exist yet -> start fresh and write normally.
/// * it exists and reads/parses fine (including a legitimately empty
///   `{"version":1,"rows":[]}`) -> update it and write normally.
/// * it exists but a read or a parse fails -> change nothing on disk. The
///   90-day retention window makes a silent overwrite here cost real
///   history, so an `UsageLog::from_json`-style "when in doubt, start over"
///   fallback is correct for a *reader* but wrong for this *writer*.
fn write_usage_log_pass(log_path: &std::path::Path, snapshot: &Value, now_ms: i64) {
    let log = match std::fs::read_to_string(log_path) {
        Ok(existing) => match usage_log::UsageLog::try_from_json(&existing) {
            Ok(log) => Some(log),
            Err(e) => {
                eprintln!(
                    "Could not parse usage log at {}: {e}. Leaving it untouched this pass.",
                    log_path.display()
                );
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(usage_log::UsageLog::new()),
        Err(e) => {
            eprintln!(
                "Could not read usage log at {}: {e}. Leaving it untouched this pass.",
                log_path.display()
            );
            None
        }
    };

    let Some(mut log) = log else {
        return;
    };
    record_usage(&mut log, snapshot, now_ms);
    if let Err(e) = usage_log::write_string_atomic(log_path, &log.to_json()) {
        eprintln!("Could not write usage log: {e}");
    }
}

/// `watch` runs until killed. `watch --once` performs a single pass and exits,
/// which is what the systemd unit's health check and manual testing use.
pub fn run_watch(args: &[String]) -> i32 {
    let once = args.iter().any(|a| a == "--once");
    loop {
        let cfg = config::load_config();
        if cfg.auto_refresh_enabled {
            let accounts = store::load_accounts().map_err(|e| e.to_string());
            // Borrowed before `accounts` moves into `env`: whether the log
            // gets touched at all depends on whether the store read
            // succeeded, per finding 1 + finding 2 — a store this pass
            // could not even read must not also clobber the log.
            let store_was_readable = accounts.is_ok();
            let env = PollEnv {
                accounts,
                fetch: &fetch_live,
                now_ms: now_ms(),
                interval_seconds: cfg.refresh_interval_seconds,
                active_email: None,
            };
            let snapshot = poll_once(&env);
            if let Err(e) = state::write_atomic(&state::state_path(), &snapshot) {
                eprintln!("Could not write state: {e}");
            }

            if store_was_readable {
                write_usage_log_pass(&usage_log::usage_log_path(), &snapshot, now_ms());
            } else {
                eprintln!("Account store was unreadable this pass; usage log left untouched.");
            }
        }
        if once {
            return 0;
        }
        // Config is re-read every pass, so an interval change takes effect on
        // the next tick without a restart.
        std::thread::sleep(std::time::Duration::from_secs(cfg.refresh_interval_seconds));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::model::{AccountPlan, AccountStatus};

    fn account(id: &str, org: Option<&str>, status: AccountStatus) -> claude_dashboard_core::model::Account {
        let mut a = crate::testsupport::account(id);
        a.org_id = org.map(str::to_string);
        a.session_key = Some("plain-key".to_string());
        a.plan = AccountPlan::Max200;
        a.status = status;
        a
    }

    #[test]
    fn a_successful_fetch_lands_under_the_store_id() {
        let fetch = |_: &str, _: &str| Ok(r#"{"five_hour":{"utilization":5}}"#.to_string());
        let env = PollEnv {
            accounts: Ok(vec![account("acc-1", Some("org-1"), AccountStatus::Active)]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let v = poll_once(&env);
        assert_eq!(v["usage"]["acc-1"]["five_hour"]["utilization"], 5);
        assert!(v["errors"].as_object().unwrap().is_empty());
        assert!(v["fatal"].is_null());
    }

    #[test]
    fn one_failing_account_does_not_stop_the_others() {
        let fetch = |org: &str, _: &str| {
            if org == "org-1" { Err("HTTP 500".to_string()) }
            else { Ok(r#"{"five_hour":{"utilization":9}}"#.to_string()) }
        };
        let env = PollEnv {
            accounts: Ok(vec![
                account("acc-1", Some("org-1"), AccountStatus::Active),
                account("acc-2", Some("org-2"), AccountStatus::Active),
            ]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let v = poll_once(&env);
        assert_eq!(v["errors"]["acc-1"], "HTTP 500");
        assert_eq!(v["usage"]["acc-2"]["five_hour"]["utilization"], 9);
        assert!(v["usage"].get("acc-1").is_none());
        assert!(v["fatal"].is_null(), "a partial failure is not fatal");
    }

    #[test]
    fn an_unparseable_payload_is_an_error_not_a_usage_entry() {
        let fetch = |_: &str, _: &str| Ok("not json".to_string());
        let env = PollEnv {
            accounts: Ok(vec![account("acc-1", Some("org-1"), AccountStatus::Active)]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let v = poll_once(&env);
        assert!(v["usage"].get("acc-1").is_none());
        assert!(v["errors"]["acc-1"].is_string());
    }

    #[test]
    fn accounts_without_an_org_id_are_listed_but_never_fetched() {
        let fetch = |_: &str, _: &str| panic!("must not fetch an account with no orgId");
        let env = PollEnv {
            accounts: Ok(vec![account("acc-1", None, AccountStatus::Active)]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let v = poll_once(&env);
        assert_eq!(v["accounts"][0]["id"], "acc-1");
        assert!(v["usage"].as_object().unwrap().is_empty());
    }

    #[test]
    fn an_empty_store_is_fatal_so_the_reader_can_say_why() {
        let fetch = |_: &str, _: &str| Ok(String::new());
        let env = PollEnv {
            accounts: Ok(vec![]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let v = poll_once(&env);
        assert_eq!(v["fatal"], "no-accounts");
    }

    #[test]
    fn the_active_claude_code_email_is_reported_as_observed() {
        let fetch = |_: &str, _: &str| Ok("{}".to_string());
        let env = PollEnv {
            accounts: Ok(vec![account("acc-1", Some("org-1"), AccountStatus::Active)]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: Some("a@example.com".to_string()),
        };
        assert_eq!(poll_once(&env)["activeClaudeCodeEmail"], "a@example.com");
    }

    #[test]
    fn a_snapshot_records_one_log_row_per_window() {
        let body = r#"{"five_hour":{"utilization":5,"resets_at":"2026-09-16T11:40:00Z"},
                       "seven_day":{"utilization":92.5,"resets_at":"2026-09-18T07:00:00Z"},
                       "limits":[{"percent":38,"resets_at":"2026-09-16T11:40:00Z",
                                  "scope":{"model":{"display_name":"Fable"}}}]}"#;
        let fetch = |_: &str, _: &str| Ok(body.to_string());
        let env = PollEnv {
            accounts: Ok(vec![account("acc-1", Some("org-1"), AccountStatus::Active)]),
            fetch: &fetch,
            now_ms: 1_789_420_000_000,
            interval_seconds: 120,
            active_email: None,
        };
        let snapshot = poll_once(&env);
        let mut log = crate::usage_log::UsageLog::new();
        record_usage(&mut log, &snapshot, 1_789_420_000_000);

        let windows: Vec<i64> = log.rows().iter().map(|r| r.w).collect();
        assert_eq!(windows, vec![0, 1, 3]);
        assert!(log.rows().iter().all(|r| r.aid == "acc-1"));
        // 92.5 -> round(9250.0); the Fable window reads `percent`, not
        // `utilization`, and must not be dropped.
        assert_eq!(log.rows()[1].u, 9250);
        assert_eq!(log.rows()[2].u, 3800);
    }

    #[test]
    fn an_account_with_an_error_contributes_no_log_rows() {
        let fetch = |_: &str, _: &str| Err("HTTP 500".to_string());
        let env = PollEnv {
            accounts: Ok(vec![account("acc-1", Some("org-1"), AccountStatus::Active)]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let snapshot = poll_once(&env);
        let mut log = crate::usage_log::UsageLog::new();
        record_usage(&mut log, &snapshot, 1000);
        assert!(log.rows().is_empty());
    }

    #[test]
    fn a_payload_missing_a_required_window_is_skipped_rather_than_half_recorded() {
        let fetch = |_: &str, _: &str| Ok(r#"{"seven_day":{"utilization":1,"resets_at":null}}"#.to_string());
        let env = PollEnv {
            accounts: Ok(vec![account("acc-1", Some("org-1"), AccountStatus::Active)]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let snapshot = poll_once(&env);
        let mut log = crate::usage_log::UsageLog::new();
        record_usage(&mut log, &snapshot, 1000);
        assert!(log.rows().is_empty(), "a five_hour-less payload must not half-record");
    }

    // --- Fix round 1 findings -------------------------------------------

    /// Finding 1: an unreadable store must publish `"store-unreadable"`, not
    /// `"no-accounts"` — the two are different situations
    /// (`contract/account-schema.md`'s "An unreadable store is not an empty
    /// store", applied here to what the daemon reports).
    #[test]
    fn an_unreadable_store_is_reported_distinctly_from_an_empty_one() {
        let fetch = |_: &str, _: &str| panic!("must not fetch when the store could not be read");
        let env = PollEnv {
            accounts: Err("permission denied".to_string()),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let v = poll_once(&env);
        assert_eq!(v["fatal"], "store-unreadable");
        assert_ne!(v["fatal"], "no-accounts");
        assert!(v["accounts"].as_array().unwrap().is_empty());
    }

    /// Finding 2, half A: a log path that does not exist yet is the
    /// legitimate "no log yet" case and must still be written.
    #[test]
    fn a_missing_log_file_still_produces_a_written_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("usage-log.json");
        let snapshot = serde_json::json!({"usage": {}});

        write_usage_log_pass(&log_path, &snapshot, 1000);

        let text = std::fs::read_to_string(&log_path).expect("a fresh log must be written");
        assert!(usage_log::UsageLog::try_from_json(&text).is_ok());
    }

    /// Finding 2, half B: a log file that exists but fails to parse must be
    /// left exactly as it was — not overwritten with a fresh empty log —
    /// because that is how 90 days of history gets destroyed by one bad
    /// read.
    #[test]
    fn an_unparseable_log_is_left_byte_identical_after_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("usage-log.json");
        std::fs::write(&log_path, "not json").unwrap();
        let snapshot = serde_json::json!({"usage": {}});

        write_usage_log_pass(&log_path, &snapshot, 1000);

        let text = std::fs::read_to_string(&log_path).unwrap();
        assert_eq!(text, "not json", "a corrupt log must not be overwritten");
    }

    /// Finding 3: `lim` must reflect `utilization >= 100.0` per window, not
    /// be hardcoded `false` — matching macOS's
    /// `BurnRateTracker.swift:33`.
    #[test]
    fn a_window_at_or_above_100_percent_records_lim_true() {
        let body = r#"{"five_hour":{"utilization":100.0,"resets_at":null},
                       "seven_day":{"utilization":50.0,"resets_at":null}}"#;
        let fetch = |_: &str, _: &str| Ok(body.to_string());
        let env = PollEnv {
            accounts: Ok(vec![account("acc-1", Some("org-1"), AccountStatus::Active)]),
            fetch: &fetch,
            now_ms: 1000,
            interval_seconds: 120,
            active_email: None,
        };
        let snapshot = poll_once(&env);
        let mut log = crate::usage_log::UsageLog::new();
        record_usage(&mut log, &snapshot, 1000);

        let five_hour = log.rows().iter().find(|r| r.w == usage_log::WINDOW_FIVE_HOUR).unwrap();
        let seven_day = log.rows().iter().find(|r| r.w == usage_log::WINDOW_SEVEN_DAY).unwrap();
        assert_eq!(five_hour.lim, 1, "utilization >= 100.0 must record lim:1");
        assert_eq!(seven_day.lim, 0, "utilization below 100.0 must record lim:0");
    }
}
