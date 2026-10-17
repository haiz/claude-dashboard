//! The refresh engine: fetch usage for every account, log it, build rows.

use std::collections::HashMap;

use claude_dashboard_core::api;
use claude_dashboard_core::burn_rate::BurnRateTracker;
use claude_dashboard_core::model::{Account, AccountPlan, AccountStatus};
use claude_dashboard_core::plan::{parse_orgs, refreshed_plan_for};
use claude_dashboard_core::extension_sources;
use claude_dashboard_core::rows::{build_rows, peak_utilization, BuildInput, DisplayRow, WindowView, sort_rows};
use claude_dashboard_core::store::{self, UsageLogStore};
use claude_dashboard_core::usage::{UsageData, UsageLimit};

#[derive(Debug, Clone, PartialEq)]
pub struct RefreshOutput {
    pub rows: Vec<DisplayRow>,
    pub peak: f64,
    /// Accounts fetched fine this refresh, for Auto Ping (macOS pings on the refresh's own
    /// fetch, never on stored usage).
    pub candidates: Vec<PingCandidate>,
}

/// An account this refresh fetched successfully, with what a ping for it needs.
#[derive(Debug, Clone, PartialEq)]
pub struct PingCandidate {
    pub account_id: String,
    pub org_id: String,
    pub session_key: String,
    pub usage: UsageData,
}

/// One account's fetch in a refresh.
pub struct Fetch {
    pub id: String,
    pub org: String,
    pub key: String,
    pub result: Result<UsageData, FetchError>,
    pub update: AccountUpdate,
}

/// The successful fetches, each with its rotated session key when the server sent one.
pub fn candidates_from(fetches: &[Fetch]) -> Vec<PingCandidate> {
    fetches
        .iter()
        .filter_map(|f| {
            let usage = f.result.as_ref().ok()?.clone();
            Some(PingCandidate {
                account_id: f.id.clone(),
                org_id: f.org.clone(),
                session_key: f.update.new_key.clone().unwrap_or_else(|| f.key.clone()),
                usage,
            })
        })
        .collect()
}

/// What wakes the refresh loop: every account, just one (the account page's Refresh), or
/// `Reschedule`: the Auto Refresh setting changed, so restart the wait with the new
/// interval without fetching anything (macOS only reschedules its timer).
/// `Some`: just these accounts, in one pass (Re-sync All refreshes the accounts it re-synced).
/// `Local`: an account field the rows show changed in the store (a pin), so re-read it and
/// re-sort without fetching (macOS `togglePin` only writes the store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nudge {
    All,
    One(String),
    Some(Vec<String>),
    Reschedule,
    Local,
}

/// The Auto Refresh timer, kept apart from manual and nudged refreshes as macOS
/// `scheduleAutoRefresh` is: only `reschedule` (a settings change) and the timer firing
/// move the next due time.
#[derive(Debug, Default)]
pub struct AutoTimer {
    due: Option<std::time::Instant>,
}

impl AutoTimer {
    /// How long to wait for the next tick; `None` when Auto Refresh is off.
    pub fn wait(&mut self, interval: Option<std::time::Duration>, now: std::time::Instant) -> Option<std::time::Duration> {
        let Some(interval) = interval else {
            self.due = None;
            return None;
        };
        let due = *self.due.get_or_insert(now + interval);
        Some(due.saturating_duration_since(now))
    }

    /// The timer fired, or the interval changed: count a full interval from `now`.
    pub fn restart(&mut self, interval: Option<std::time::Duration>, now: std::time::Instant) {
        self.due = interval.map(|i| now + i);
    }
}

/// The 60 s poll of the active account while auto switch is on and it is near a limit
/// (macOS `updateAutoSwitchPoll`): the first poll comes a full period after it starts.
#[derive(Debug, Default)]
pub struct FastPoll {
    due: Option<std::time::Instant>,
}

impl FastPoll {
    pub const PERIOD: std::time::Duration = std::time::Duration::from_secs(crate::auto_switch::FAST_POLL_S);

    /// Starts the poll (keeping a running one's schedule) or stops it.
    pub fn update(&mut self, on: bool, now: std::time::Instant) {
        if !on {
            self.due = None;
        } else if self.due.is_none() {
            self.due = Some(now + Self::PERIOD);
        }
    }

    /// Time left to the next poll; `None` while stopped.
    pub fn wait(&self, now: std::time::Instant) -> Option<std::time::Duration> {
        self.due.map(|d| d.saturating_duration_since(now))
    }

    /// True when the poll is due; schedules the next one a period from `now`.
    pub fn fire(&mut self, now: std::time::Instant) -> bool {
        match self.due {
            Some(d) if d <= now => {
                self.due = Some(now + Self::PERIOD);
                true
            }
            _ => false,
        }
    }
}

/// Windows whose reset time passed more than 10 s ago on an active row, as
/// `(account, window, resets_at)`; macOS `checkResetTriggers` refreshes for these so the
/// gauges move on as soon as a window resets. The caller fires once per triple.
pub fn passed_resets(rows: &[DisplayRow], now_unix_s: f64) -> Vec<(String, i64, i64)> {
    let threshold = now_unix_s - 10.0;
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r.status == AccountStatus::Active) {
        for (w, view) in [(WINDOW_FIVE_HOUR, &r.five_hour), (WINDOW_SEVEN_DAY, &r.seven_day)] {
            if let Some(at) = view.as_ref().and_then(|v| v.resets_at_unix).filter(|&at| at <= threshold) {
                out.push((r.account_id.clone(), w, at as i64));
            }
        }
    }
    out
}

/// `Nudge::Local`: copies the pin flags from `accounts` and the Claude Code badge from
/// `.claude.json` onto `prev`, and re-sorts. No fetch.
pub fn relabel(prev: &[DisplayRow], accounts: &[Account], now_unix_s: f64) -> RefreshOutput {
    let active = claude_dashboard_core::claude_code::active_email();
    relabel_with(prev, accounts, active.as_deref(), now_unix_s)
}

pub fn relabel_with(prev: &[DisplayRow], accounts: &[Account], active_email: Option<&str>, now_unix_s: f64) -> RefreshOutput {
    let mut rows = prev.to_vec();
    for r in rows.iter_mut() {
        if let Some(a) = accounts.iter().find(|a| a.id == r.account_id) {
            r.is_pinned = a.is_pinned;
            r.is_active_claude_code = active_email.is_some_and(|e| a.email.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(e)));
        }
    }
    sort_rows(&mut rows, now_unix_s);
    let peak = peak_utilization(&rows);
    RefreshOutput { rows, peak, candidates: Vec::new() }
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

/// Refreshes the accounts `ids` (macOS `refreshAll(only:)`) and keeps every other row from
/// `prev` as it was (their burn projections and error lines included). A failure to read
/// the store, or none of `ids` still existing, leaves `prev` unchanged.
pub fn refresh_some(ids: &[String], prev: &[DisplayRow], now_unix_s: f64) -> RefreshOutput {
    let fresh = store::load_accounts()
        .map_err(|e| e.to_string())
        .and_then(|all| {
            let some: Vec<Account> = all.into_iter().filter(|a| ids.contains(&a.id)).collect();
            if some.is_empty() { Err(format!("accounts {ids:?} not found")) } else { Ok(some) }
        })
        .map(|some| fetch_and_build(&some, now_unix_s));
    match fresh {
        Ok((mut rows, candidates)) => {
            backfill_errored_rows(prev, &mut rows);
            let merged = rows.into_iter().fold(prev.to_vec(), |acc, row| merge_one(&acc, row, now_unix_s));
            let peak = peak_utilization(&merged);
            RefreshOutput { rows: merged, peak, candidates }
        }
        Err(e) => merge_errors(prev, Err(e), now_unix_s),
    }
}

/// Ok -> the fresh output (backfilled rows re-sorted by their last-good usage); Err -> the
/// previous rows, unchanged.
pub fn merge_errors(prev: &[DisplayRow], fresh: Result<RefreshOutput, String>, now_unix_s: f64) -> RefreshOutput {
    match fresh {
        Ok(mut out) => {
            backfill_errored_rows(prev, &mut out.rows);
            sort_rows(&mut out.rows, now_unix_s);
            out.peak = peak_utilization(&out.rows);
            out
        }
        Err(e) => {
            eprintln!("refresh failed, keeping previous rows: {e}");
            RefreshOutput { rows: prev.to_vec(), peak: peak_utilization(prev), candidates: Vec::new() }
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

/// A row that was not fetched fine this time (errored, or expired and so skipped) and has
/// no window data of its own keeps the previous row's last-good windows, as macOS keeps
/// `usage` across a failed fetch, so its gauges do not blank.
fn backfill_errored_rows(prev: &[DisplayRow], fresh: &mut [DisplayRow]) {
    for row in fresh.iter_mut() {
        let unfetched = row.error.is_some() || row.status != AccountStatus::Active;
        if !unfetched || row.five_hour.is_some() || row.seven_day.is_some() || row.fable.is_some() {
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

/// Why one account's usage fetch failed. `Auth` (HTTP 401/403) marks the account
/// expired, as macOS `UsageAPIError.authExpired` does; anything else is shown on its card.
#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    Auth,
    Other(String),
}

/// `core::api` keeps 401/403 as plain HTTP errors (the helper prints them); the app
/// reads them as an expired session.
pub fn classify(e: &api::ApiError) -> FetchError {
    match e {
        api::ApiError::HttpError(401 | 403) => FetchError::Auth,
        other => FetchError::Other(other.to_string()),
    }
}

/// macOS `fetchWithRetry`: up to `attempts` tries, `delay` apart; an auth failure is final.
pub fn with_retry<T>(
    attempts: u32,
    delay: std::time::Duration,
    mut op: impl FnMut() -> Result<T, FetchError>,
    sleep: impl Fn(std::time::Duration),
) -> Result<T, FetchError> {
    let mut last = FetchError::Other("no attempt".into());
    for attempt in 0..attempts {
        match op() {
            Ok(v) => return Ok(v),
            Err(FetchError::Auth) => return Err(FetchError::Auth),
            Err(e) => last = e,
        }
        if attempt + 1 < attempts {
            sleep(delay);
        }
    }
    Err(last)
}

fn fetch_one(org_id: &str, session_key: &str) -> Result<Fetched, FetchError> {
    let resp = api::usage_raw(org_id, session_key).map_err(|e| classify(&e))?;
    if resp.body.is_empty() {
        return Err(FetchError::Other("Empty response.".into()));
    }
    // macOS shows a decoding failure as a transient read error.
    let usage = UsageData::decode(&resp.body)
        .map_err(|_| FetchError::Other("Temporary read error. Try refreshing.".into()))?;
    Ok((usage, resp.new_session_key))
}

/// What one refresh learned about an account, written back to the store and to the
/// in-memory copy the rows are built from (macOS `refreshAll`'s update step).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountUpdate {
    /// 401/403: the session is gone.
    pub expired: bool,
    /// Fetched fine: active again, `last_synced` = now.
    pub synced: bool,
    pub new_key: Option<String>,
    /// From `/api/organizations` (`contract/cases/plan-refresh.json`); `None` leaves it.
    pub plan: Option<AccountPlan>,
    /// From `/api/account` when the record has no uuid yet: (uuid, email).
    pub identity: Option<(String, Option<String>)>,
}

/// Applies `u` to `a`; the session key is the caller's (it is sealed only on disk).
/// Identity only fills blanks and never touches `org_id`, as on macOS.
pub fn apply_update(a: &mut Account, u: &AccountUpdate, now_unix_s: f64) {
    if u.expired {
        a.status = AccountStatus::Expired;
    }
    if u.synced {
        a.status = AccountStatus::Active;
        a.last_synced = Some(now_unix_s - REFERENCE_EPOCH_OFFSET);
    }
    if let Some(p) = &u.plan {
        a.plan = p.clone();
    }
    if let Some((uuid, email)) = &u.identity {
        if a.account_uuid.is_none() {
            a.account_uuid = Some(uuid.clone());
        }
        if a.email.is_none() {
            a.email = email.clone();
        }
    }
}

/// One account's fetch plus its best-effort plan and identity lookups (on success only).
fn refresh_account(a: &Account, key: &str, org: &str) -> (Result<UsageData, FetchError>, AccountUpdate) {
    let mut update = AccountUpdate::default();
    let fetched = with_retry(3, std::time::Duration::from_secs(1), || fetch_one(org, key), std::thread::sleep);
    let usage = match fetched {
        Ok((usage, new_key)) => {
            update.synced = true;
            update.new_key = new_key;
            let orgs = api::fetch_organizations(key).map(|b| parse_orgs(&b)).unwrap_or_default();
            update.plan = refreshed_plan_for(a, &orgs);
            if a.account_uuid.is_none() {
                update.identity = api::fetch_account(key)
                    .ok()
                    .and_then(|b| api::parse_account(&b))
                    .map(|i| (i.uuid, i.email));
            }
            Ok(usage)
        }
        Err(e) => {
            update.expired = e == FetchError::Auth;
            Err(e)
        }
    };
    (usage, update)
}

/// The extension install bindings (empty if unreadable).
fn load_sources() -> extension_sources::ExtensionSources {
    extension_sources::load(&extension_sources::sources_path()).unwrap_or_default()
}

/// Reference-date (2001-01-01) offset from the Unix epoch, as `Account.last_synced` uses.
const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0;

/// Writes every account's update (status, sync stamp, rotated key, plan, identity) in one
/// locked load-modify-save (shared with the bridge writers), after all fetches so there
/// are no write races.
fn persist_refresh(updates: &HashMap<String, AccountUpdate>, now_unix_s: f64) {
    if updates.values().all(|u| *u == AccountUpdate::default()) {
        return;
    }
    let Ok(_lock) = store::lock_store() else { return };
    let Ok((mut accounts, _)) = store::load_accounts_for_write() else { return };
    for a in accounts.iter_mut() {
        if let Some(u) = updates.get(&a.id) {
            apply_update(a, u, now_unix_s);
            if let Some(k) = &u.new_key {
                a.session_key = Some(store::encrypt_session_key(k));
            }
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
    let (rows, candidates) = fetch_and_build(&accounts, now_unix_s);
    let peak = peak_utilization(&rows);
    Ok(RefreshOutput { rows, peak, candidates })
}

/// The `rat` a reading is logged under: its reset time, or now + the window length when
/// the API sent none (macOS records `resetsAt ?? now + window`, `contract/usage-log.md`).
pub fn log_resets_at(l: &UsageLimit, total_s: f64, now_unix_s: f64) -> f64 {
    l.resets_at.map(|r| r as f64).unwrap_or(now_unix_s + total_s)
}

/// Fetches usage for `accounts` (one thread each), writes back what it learned, logs the
/// readings and builds their sorted rows. Like macOS `refreshAll`, expired accounts and
/// accounts with no key or org are not fetched (their rows carry no fresh usage).
fn fetch_and_build(accounts: &[Account], now_unix_s: f64) -> (Vec<DisplayRow>, Vec<PingCandidate>) {
    let results: Vec<Fetch> = std::thread::scope(|s| {
        let handles: Vec<_> = accounts
            .iter()
            .filter(|a| a.status != AccountStatus::Expired)
            .filter_map(|a| {
                let key = a.session_key.as_deref().and_then(store::decrypt_session_key)?;
                let org = a.org_id.clone()?;
                Some(s.spawn(move || {
                    let (result, update) = refresh_account(a, &key, &org);
                    Fetch { id: a.id.clone(), org, key, result, update }
                }))
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });

    let candidates = candidates_from(&results);
    let mut usage_by_account: HashMap<String, UsageData> = HashMap::new();
    let mut errors: HashMap<String, String> = HashMap::new();
    let mut updates: HashMap<String, AccountUpdate> = HashMap::new();
    for Fetch { id, result: r, update: u, .. } in results {
        match r {
            Ok(usage) => {
                usage_by_account.insert(id.clone(), usage);
            }
            // An expired session shows the expired card, not an error line.
            Err(FetchError::Auth) => {}
            Err(FetchError::Other(e)) => {
                errors.insert(id.clone(), e);
            }
        }
        updates.insert(id, u);
    }

    persist_refresh(&updates, now_unix_s);
    let accounts: Vec<Account> = accounts
        .iter()
        .cloned()
        .map(|mut a| {
            if let Some(u) = updates.get(&a.id) {
                apply_update(&mut a, u, now_unix_s);
            }
            a
        })
        .collect();

    if let Ok(mut log) = UsageLogStore::open() {
        for (id, u) in &usage_by_account {
            let mut rec = |w: i64, l: &UsageLimit, total_s: f64| {
                log.record(id, w, log_resets_at(l, total_s, now_unix_s), l.utilization, l.utilization >= 100.0);
            };
            rec(WINDOW_FIVE_HOUR, &u.five_hour, 18000.0);
            rec(WINDOW_SEVEN_DAY, &u.seven_day, 604800.0);
            if let Some(f) = &u.fable {
                rec(WINDOW_FABLE, f, 604800.0);
            }
        }
    }

    let sources = load_sources();
    let active_email = claude_dashboard_core::claude_code::active_email();
    let rows = build_rows(BuildInput {
        accounts: &accounts,
        usage_by_account: &usage_by_account,
        errors: &errors,
        sources: &sources,
        now_unix_s,
        active_claude_code_email: active_email.as_deref(),
    });
    (rows, candidates)
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
    fn fast_poll_arms_fires_and_disarms() {
        use std::time::{Duration, Instant};
        let t = Instant::now();
        let mut p = FastPoll::default();
        assert_eq!(p.wait(t), None);
        assert!(!p.fire(t));
        p.update(true, t);
        assert_eq!(p.wait(t), Some(Duration::from_secs(60)));
        p.update(true, t + Duration::from_secs(30));
        assert_eq!(p.wait(t + Duration::from_secs(30)), Some(Duration::from_secs(30)), "a running poll keeps its schedule");
        assert!(!p.fire(t + Duration::from_secs(59)));
        assert!(p.fire(t + Duration::from_secs(60)));
        assert_eq!(p.wait(t + Duration::from_secs(60)), Some(Duration::from_secs(60)));
        p.update(false, t + Duration::from_secs(61));
        assert_eq!(p.wait(t + Duration::from_secs(61)), None);
        assert!(!p.fire(t + Duration::from_secs(500)));
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
        RefreshOutput { rows, peak, candidates: Vec::new() }
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
        let merged = merge_errors(&prev, Err("offline".into()), NOW);
        assert_eq!(merged.rows, prev);
        assert_eq!(merged.peak, 70.0);
    }

    #[test]
    fn whole_refresh_failure_keeps_last_good_rows() {
        let prev = rows(&[("a", "old")]);
        let merged = merge_errors(&prev, Err("store unreadable".into()), NOW);
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
        let merged = merge_errors(&prev, Ok(out(fresh)), NOW);
        let b = merged.rows.iter().find(|r| r.account_id == "b").unwrap();
        assert_eq!(b.error.as_deref(), Some("HTTP 500"));
        assert_eq!(b.five_hour.as_ref().map(|w| w.utilization), Some(70.0));
        assert!(b.seven_day.is_some());
        assert_eq!(merged.peak, 70.0);
        assert_eq!(merged.rows.len(), 3);
    }

    fn fetch(id: &str, result: Result<UsageData, FetchError>, new_key: Option<&str>) -> Fetch {
        Fetch {
            id: id.into(),
            org: format!("org-{id}"),
            key: format!("key-{id}"),
            result,
            update: AccountUpdate { new_key: new_key.map(String::from), ..Default::default() },
        }
    }

    #[test]
    fn candidates_only_from_successful_fetches() {
        let fetches = [
            fetch("a", Ok(usage(10.0)), None),
            fetch("b", Err(FetchError::Auth), None),
            fetch("c", Err(FetchError::Other("HTTP 500".into())), None),
        ];
        let c = candidates_from(&fetches);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0],
            PingCandidate { account_id: "a".into(), org_id: "org-a".into(), session_key: "key-a".into(), usage: usage(10.0) }
        );
    }

    #[test]
    fn candidate_uses_rotated_key() {
        let c = candidates_from(&[fetch("a", Ok(usage(1.0)), Some("rotated"))]);
        assert_eq!(c[0].session_key, "rotated");
    }

    #[test]
    fn auto_timer_ignores_nudges_and_restarts_only_when_told() {
        use std::time::{Duration, Instant};
        let (t0, five) = (Instant::now(), Some(Duration::from_secs(300)));
        let mut t = AutoTimer::default();
        assert_eq!(t.wait(five, t0), Some(Duration::from_secs(300)));
        // A manual refresh 100 s in does not push the tick back.
        assert_eq!(t.wait(five, t0 + Duration::from_secs(100)), Some(Duration::from_secs(200)));
        t.restart(five, t0 + Duration::from_secs(300));
        assert_eq!(t.wait(five, t0 + Duration::from_secs(300)), Some(Duration::from_secs(300)));
        assert_eq!(t.wait(None, t0), None, "off");
        assert_eq!(t.wait(five, t0), Some(Duration::from_secs(300)), "back on: a full interval");
    }

    #[test]
    fn passed_resets_lists_active_windows_past_the_grace() {
        let mut rs = rows(&[]);
        for r in rs.iter_mut() {
            r.five_hour.as_mut().unwrap().resets_at_unix = Some(NOW - 11.0);
            r.seven_day.as_mut().unwrap().resets_at_unix = Some(NOW - 5.0);
        }
        rs.iter_mut().find(|r| r.account_id == "c").unwrap().status = AccountStatus::Expired;
        let mut got = passed_resets(&rs, NOW);
        got.sort();
        assert_eq!(got, vec![("a".into(), 0, (NOW - 11.0) as i64), ("b".into(), 0, (NOW - 11.0) as i64)]);
    }

    #[test]
    fn relabel_moves_the_claude_code_badge() {
        let prev = rows(&[]);
        let mut accts = vec![acct("a"), acct("b"), acct("c")];
        accts[1].email = Some("b@x.com".into());
        let out = relabel_with(&prev, &accts, Some("B@X.com"), NOW);
        assert!(out.rows.iter().find(|r| r.account_id == "b").unwrap().is_active_claude_code);
        assert_eq!(out.rows.iter().filter(|r| r.is_active_claude_code).count(), 1);
    }

    #[test]
    fn relabel_moves_the_new_pin_first_without_fetching() {
        let prev = rows(&[]);
        let mut accts = vec![acct("a"), acct("b"), acct("c")];
        accts[2].is_pinned = true;
        let out = relabel(&prev, &accts, NOW);
        assert_eq!(out.rows[0].account_id, "c");
        assert!(out.rows[0].is_pinned);
        assert_eq!(out.rows.iter().filter(|r| r.is_pinned).count(), 1);
    }

    #[test]
    fn auth_statuses_classify_as_expired_others_keep_their_text() {
        use api::ApiError;
        assert_eq!(classify(&ApiError::HttpError(401)), FetchError::Auth);
        assert_eq!(classify(&ApiError::HttpError(403)), FetchError::Auth);
        assert_eq!(classify(&ApiError::HttpError(500)), FetchError::Other("HTTP 500".into()));
        assert_eq!(classify(&ApiError::Timeout), FetchError::Other("request timed out".into()));
    }

    #[test]
    fn retry_tries_three_times_but_stops_on_auth() {
        use std::cell::Cell;
        let (calls, sleeps) = (Cell::new(0), Cell::new(0));
        let d = std::time::Duration::from_secs(1);
        let r: Result<(), _> = with_retry(3, d, || {
            calls.set(calls.get() + 1);
            Err(FetchError::Other("boom".into()))
        }, |_| sleeps.set(sleeps.get() + 1));
        assert_eq!(r, Err(FetchError::Other("boom".into())));
        assert_eq!((calls.get(), sleeps.get()), (3, 2));

        calls.set(0);
        let r: Result<(), _> = with_retry(3, d, || {
            calls.set(calls.get() + 1);
            Err(FetchError::Auth)
        }, |_| {});
        assert_eq!(r, Err(FetchError::Auth));
        assert_eq!(calls.get(), 1);

        calls.set(0);
        let r = with_retry(3, d, || {
            calls.set(calls.get() + 1);
            if calls.get() < 2 { Err(FetchError::Other("x".into())) } else { Ok(7) }
        }, |_| {});
        assert_eq!(r, Ok(7));
    }

    #[test]
    fn updates_mark_expired_or_heal_and_fill_identity_blanks_only() {
        let mut a = acct("a");
        a.status = AccountStatus::Error;
        apply_update(&mut a, &AccountUpdate { expired: true, ..Default::default() }, NOW);
        assert_eq!(a.status, AccountStatus::Expired);

        let u = AccountUpdate {
            synced: true,
            plan: Some(AccountPlan::Max20x),
            identity: Some(("uuid-1".into(), Some("a@x.com".into()))),
            ..Default::default()
        };
        apply_update(&mut a, &u, NOW);
        assert_eq!(a.status, AccountStatus::Active);
        assert_eq!(a.last_synced_unix(), Some(NOW));
        assert_eq!(a.plan, AccountPlan::Max20x);
        assert_eq!(a.account_uuid.as_deref(), Some("uuid-1"));
        assert_eq!(a.email.as_deref(), Some("a@x.com"));

        let again = AccountUpdate { identity: Some(("uuid-2".into(), Some("b@x.com".into()))), ..Default::default() };
        apply_update(&mut a, &again, NOW);
        assert_eq!(a.account_uuid.as_deref(), Some("uuid-1"), "never overwritten");
        assert_eq!(a.email.as_deref(), Some("a@x.com"));
    }

    #[test]
    fn log_resets_at_falls_back_to_now_plus_window() {
        let l = UsageLimit { utilization: 1.0, resets_at: None };
        assert_eq!(log_resets_at(&l, 18000.0, NOW), NOW + 18000.0);
        let l = UsageLimit { utilization: 1.0, resets_at: Some(42) };
        assert_eq!(log_resets_at(&l, 18000.0, NOW), 42.0);
    }

    #[test]
    fn expired_row_keeps_last_good_windows_and_errored_row_keeps_its_place() {
        let prev = rows(&[]);
        let mut fresh = rows(&[("b", "HTTP 500")]);
        let a = fresh.iter_mut().find(|r| r.account_id == "a").unwrap();
        a.status = AccountStatus::Expired;
        a.five_hour = None;
        a.seven_day = None;
        let b = fresh.iter_mut().find(|r| r.account_id == "b").unwrap();
        b.five_hour = None;
        b.seven_day = None;
        let merged = merge_errors(&prev, Ok(out(fresh)), NOW);
        let a = merged.rows.iter().find(|r| r.account_id == "a").unwrap();
        assert_eq!(a.five_hour.as_ref().map(|w| w.utilization), Some(40.0));
        // b (70%, errored) still leads on its last-good usage; expired a sinks.
        assert_eq!(merged.rows.iter().map(|r| r.account_id.as_str()).collect::<Vec<_>>(), ["b", "c", "a"]);
    }

    #[test]
    fn merge_errors_takes_fresh_on_success() {
        let prev = rows(&[]);
        let fresh = out(vec![prev[0].clone()]);
        let merged = merge_errors(&prev, Ok(fresh.clone()), NOW);
        assert_eq!(merged, fresh);
    }

    #[test]
    fn refresh_merges_errors_without_dropping_rows() {
        let merged = merge_errors(&[], Ok(out(rows(&[("a", "HTTP 500")]))), NOW);
        assert_eq!(merged.rows.len(), 3);
        let a = merged.rows.iter().find(|r| r.account_id == "a").unwrap();
        assert_eq!(a.error.as_deref(), Some("HTTP 500"));
        assert_eq!(a.five_hour.as_ref().map(|w| w.utilization), Some(40.0));
        assert_eq!(merged.rows.iter().filter(|r| r.error.is_none()).count(), 2);
    }
}
