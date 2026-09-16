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
    pub accounts: Vec<Account>,
    pub fetch: &'a dyn Fn(&str, &str) -> Result<String, String>,
    pub now_ms: i64,
    pub interval_seconds: u64,
    pub active_email: Option<String>,
}

/// One poll pass, with every side effect injected so it is testable without a
/// browser, a keychain, a network or a clock.
pub fn poll_once(env: &PollEnv) -> Value {
    let mut usage: BTreeMap<String, Value> = BTreeMap::new();
    let mut errors: BTreeMap<String, String> = BTreeMap::new();

    for account in &env.accounts {
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

    let fatal = if env.accounts.is_empty() { Some("no-accounts") } else { None };

    state::build_state(
        &env.accounts,
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

        let mut write = |w: i64, utilization: f64, resets_at: Option<i64>| {
            log.record(
                account_id,
                w,
                resets_at.unwrap_or(0) * 1000,
                utilization,
                false,
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

/// `watch` runs until killed. `watch --once` performs a single pass and exits,
/// which is what the systemd unit's health check and manual testing use.
pub fn run_watch(args: &[String]) -> i32 {
    let once = args.iter().any(|a| a == "--once");
    loop {
        let cfg = config::load_config();
        if cfg.auto_refresh_enabled {
            let accounts = store::load_accounts().unwrap_or_default();
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

            // The log is read-modify-write rather than append: the
            // compression policy needs the existing rows for the same
            // (aid, w, rat) triple to decide what to drop.
            let log_path = usage_log::usage_log_path();
            let existing = std::fs::read_to_string(&log_path).unwrap_or_default();
            let mut log = usage_log::UsageLog::from_json(&existing);
            record_usage(&mut log, &snapshot, now_ms());
            if let Err(e) = usage_log::write_string_atomic(&log_path, &log.to_json()) {
                eprintln!("Could not write usage log: {e}");
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
            accounts: vec![account("acc-1", Some("org-1"), AccountStatus::Active)],
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
            accounts: vec![
                account("acc-1", Some("org-1"), AccountStatus::Active),
                account("acc-2", Some("org-2"), AccountStatus::Active),
            ],
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
            accounts: vec![account("acc-1", Some("org-1"), AccountStatus::Active)],
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
            accounts: vec![account("acc-1", None, AccountStatus::Active)],
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
            accounts: vec![],
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
            accounts: vec![account("acc-1", Some("org-1"), AccountStatus::Active)],
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
            accounts: vec![account("acc-1", Some("org-1"), AccountStatus::Active)],
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
            accounts: vec![account("acc-1", Some("org-1"), AccountStatus::Active)],
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
            accounts: vec![account("acc-1", Some("org-1"), AccountStatus::Active)],
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
}
