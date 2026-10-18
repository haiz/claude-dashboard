//! Auto Switch (macOS `AutoSwitchController` + `DashboardViewModel+AutoSwitch`): runs the
//! planner after each refresh and performs its decision. Holds what the planner must not:
//! the lock and the hold (in memory only), the 10-minute failure cooldown, the footer event
//! and the Switch Log rows. The switch itself is the caller's closure, so this never opens a
//! browser and never prompts.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex, MutexGuard};

use claude_dashboard_core::auto_switch::{
    self as planner, Account, Decision, Hold, Lock, Priority, Reason, Snapshot, StayReason, Thresholds, Usage,
};
use claude_dashboard_core::auto_switch_log::{NewEntry, Outcome};
use claude_dashboard_core::cc_switch::switcher::SwitchError;
use claude_dashboard_core::critical::CriticalLevel;
use claude_dashboard_core::auto_switch::Window;
use claude_dashboard_core::cc_switch::switcher::Availability;
use claude_dashboard_core::critical::{CriticalAssessment, CriticalParameters};
use claude_dashboard_core::model::AccountStatus;
use claude_dashboard_core::rows::DisplayRow;
use claude_dashboard_core::settings::Settings;
use claude_dashboard_core::store::SeriesPoint;

pub const FAILURE_COOLDOWN_S: f64 = 600.0;
/// The fast poll's period while the active account is in the warning zone.
pub const FAST_POLL_S: u64 = 60;

/// The footer's report of the last evaluation that did something.
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    Switched(Reason),
    Failed(String),
    NoCandidate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    pub account_name: Option<String>,
    pub date_unix: f64,
}

impl Event {
    pub fn text(&self) -> String {
        let name = self.account_name.as_deref().unwrap_or("account");
        match &self.kind {
            EventKind::Switched(Reason::PriorityReset) => format!("Switched to {name} after its 5h reset"),
            EventKind::Switched(Reason::Critical(level)) => format!("Switched to {name}: {} account", level.label()),
            EventKind::Switched(_) => format!("Auto-switched to {name}"),
            EventKind::Failed(reason) => format!("Could not switch to {name}: {reason}"),
            EventKind::NoCandidate => "No account to switch to".into(),
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(self.kind, EventKind::Failed(_))
    }
}

/// What the evaluation reads from (and writes back to) the settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub thresholds: Thresholds,
    pub priority: Option<Priority>,
}

impl Config {
    pub fn from_settings(s: &Settings) -> Config {
        Config {
            enabled: s.auto_switch_enabled,
            thresholds: thresholds_of(s),
            priority: s.auto_switch_priority_id.clone().map(|id| Priority { account_id: id, baseline: s.auto_switch_priority_baseline }),
        }
    }
}

pub fn thresholds_of(s: &Settings) -> Thresholds {
    Thresholds { five_hour: s.auto_switch_five_hour as f64, seven_day: s.auto_switch_seven_day as f64 }
}

#[derive(Debug, Default, Clone)]
pub struct Controller {
    pub last_event: Option<Event>,
    pub lock: Option<Lock>,
    pub hold: Option<Hold>,
    cooldown_until: HashMap<String, f64>,
    /// Set when a switch fails because of Claude Code's current login, not the target:
    /// every target would fail the same way, so auto switch pauses as a whole.
    paused_until: Option<f64>,
    /// "No candidate" repeats on every refresh until something changes; it is logged once.
    logged_no_candidate: bool,
}

static CONTROLLER: LazyLock<Mutex<Controller>> = LazyLock::new(Default::default);

pub fn controller() -> MutexGuard<'static, Controller> {
    CONTROLLER.lock().unwrap_or_else(|e| e.into_inner())
}

impl Controller {
    /// A manual Switch to `id`: ends the lock and holds Claude Code there until its 5h window
    /// resets, so the critical rules do not undo the user's choice on the next refresh.
    pub fn note_manual_switch(&mut self, id: &str, usage: Option<Usage>, now: f64) {
        self.lock = None;
        self.hold = Some(make_hold(id, usage, now));
    }

    /// True while auto switch is on and the active account is in the warning zone.
    pub fn should_fast_poll(&self, cfg: &Config, accounts: &[Account]) -> bool {
        let Some(active) = accounts.iter().find(|a| a.is_active) else { return false };
        cfg.enabled && active.is_fresh && active.usage.is_some_and(|u| planner::is_in_warning_zone(&u, &cfg.thresholds))
    }

    /// Returns the account switched to. `cfg.priority` may change (dropped, or its baseline
    /// moved): the caller saves it. `log` receives the Switch Log rows.
    pub fn evaluate(
        &mut self,
        cfg: &mut Config,
        accounts: &[Account],
        now: f64,
        pick_index: impl FnMut(usize) -> usize,
        log: &mut dyn FnMut(NewEntry),
        perform: impl FnOnce(&str) -> Result<(), SwitchError>,
    ) -> Option<String> {
        update_priority(cfg, accounts, now);
        if !cfg.enabled {
            return None;
        }
        if self.paused_until.is_some_and(|p| p > now) {
            return None;
        }
        self.paused_until = None;
        self.cooldown_until.retain(|_, until| *until > now);
        let active = accounts.iter().find(|a| a.is_active);
        self.lock = planner::updated_lock(self.lock.take(), active);
        self.hold = updated_hold(self.hold.take(), active, now);
        let snapshot = Snapshot {
            accounts: accounts.to_vec(),
            thresholds: cfg.thresholds,
            lock: self.lock.clone(),
            hold: self.hold.clone(),
            priority: cfg.priority.clone(),
            excluded: self.cooldown_until.keys().cloned().collect::<HashSet<String>>(),
            now,
        };
        let decision = planner::decide(&snapshot, pick_index);
        if decision != Decision::Stay(StayReason::NoCandidate) {
            self.logged_no_candidate = false;
        }
        let from_name = active.map(|a| a.display_name.clone());
        match decision {
            Decision::Stay(StayReason::NoCandidate) => {
                self.last_event = Some(Event { kind: EventKind::NoCandidate, account_name: None, date_unix: now });
                if !self.logged_no_candidate {
                    self.logged_no_candidate = true;
                    log(NewEntry {
                        date_unix: now,
                        outcome: Outcome::NoCandidate,
                        from_name,
                        to_name: None,
                        reason: self.trigger_text(active, cfg),
                        detail: None,
                    });
                }
                None
            }
            Decision::Stay(_) => None,
            Decision::SwitchTo(id, reason) => {
                let target = accounts.iter().find(|a| a.id == id);
                let to_name = target.map(|t| t.display_name.clone());
                let reason_text = match reason {
                    Reason::PriorityReset => "Ticked account's 5h window reset".to_string(),
                    Reason::Critical(level) => {
                        let kind = if level == CriticalLevel::High { "High critical" } else { "Critical" };
                        let waste = percent(target.map_or(0.0, |t| t.critical.waste));
                        format!("{kind} account's 5h window is free, {waste} of 7d would go unused")
                    }
                    Reason::Threshold(tier) => format!("{}, tier {tier}", self.trigger_text(active, cfg)),
                };
                if let Err(e) = perform(&id) {
                    if is_active_login_error(&e) {
                        self.paused_until = Some(now + FAILURE_COOLDOWN_S);
                    } else {
                        self.cooldown_until.insert(id.clone(), now + FAILURE_COOLDOWN_S);
                    }
                    self.last_event = Some(Event { kind: EventKind::Failed(describe(&e)), account_name: to_name.clone(), date_unix: now });
                    log(NewEntry { date_unix: now, outcome: Outcome::Failed, from_name, to_name, reason: reason_text, detail: Some(describe(&e)) });
                    return None;
                }
                self.lock = target.and_then(|t| t.usage).and_then(|u| planner::make_lock(&id, &u, &cfg.thresholds));
                self.hold = match reason {
                    Reason::Critical(_) => Some(make_hold(&id, target.and_then(|t| t.usage), now)),
                    _ => None,
                };
                self.last_event = Some(Event { kind: EventKind::Switched(reason), account_name: to_name.clone(), date_unix: now });
                log(NewEntry {
                    date_unix: now,
                    outcome: Outcome::Switched,
                    from_name,
                    to_name,
                    reason: reason_text,
                    detail: target.map(|t| format!("{}: {}", t.display_name, usage_text(t))),
                });
                Some(id)
            }
        }
    }

    /// The active account's windows at or over their limit (the lock's 100 included), e.g.
    /// "5h 96% >= 95%". Falls back to both windows' usage when none is over.
    fn trigger_text(&self, active: Option<&Account>, cfg: &Config) -> String {
        let Some((active, u)) = active.and_then(|a| a.usage.map(|u| (a, u))) else {
            return "Claude Code's account has no usage".into();
        };
        let limits = planner::effective_thresholds(&active.id, &cfg.thresholds, self.lock.as_ref());
        let over: Vec<String> = [("5h", u.five_hour.utilization, limits.five_hour), ("7d", u.seven_day.utilization, limits.seven_day)]
            .into_iter()
            .filter(|(_, v, l)| v >= l)
            .map(|(w, v, l)| format!("{w} {} >= {}", percent(v), percent(l)))
            .collect();
        if over.is_empty() { usage_text(active) } else { over.join(", ") }
    }
}

/// Until the account's 5h reset, or a full window from now when its window has not started
/// (or the card still shows a reset already past).
fn make_hold(id: &str, usage: Option<Usage>, now: f64) -> Hold {
    let resets = usage.and_then(|u| u.five_hour.resets_at).filter(|r| *r > now);
    Hold { account_id: id.to_string(), until: resets.unwrap_or(now + planner::FIVE_HOUR_WINDOW_S) }
}

/// The hold covers the first 5h window the held account runs after the switch. While that
/// window has not started, `until` slides a full window ahead; once it starts before
/// `until`, `until` becomes its real reset. Dropped once Claude Code uses another account;
/// kept when no active account is detected (a failed read of Claude Code's login).
fn updated_hold(hold: Option<Hold>, active: Option<&Account>, now: f64) -> Option<Hold> {
    let mut hold = hold?;
    let Some(active) = active else { return Some(hold) };
    if active.id != hold.account_id {
        return None;
    }
    let Some(u) = active.usage.filter(|_| active.is_fresh && now < hold.until) else { return Some(hold) };
    match u.five_hour.resets_at {
        Some(resets) => {
            if resets - planner::FIVE_HOUR_WINDOW_S < hold.until {
                hold.until = resets;
            }
        }
        None => hold.until = hold.until.max(now + planner::FIVE_HOUR_WINDOW_S),
    }
    Some(hold)
}

/// Drops a tick on an account the store no longer has (macOS `updatePriority`'s removed-account
/// rule). Decided against the store, not the rows: a refresh that read no rows is not a removal.
pub fn drop_deleted_home(cfg: &mut Config, stored_ids: &[String]) {
    if cfg.priority.as_ref().is_some_and(|p| !stored_ids.contains(&p.account_id)) {
        cfg.priority = None;
    }
}

/// While the ticked account is active, moves its baseline to the window Claude Code uses, so
/// leaving it waits for that window's reset.
fn update_priority(cfg: &mut Config, accounts: &[Account], now: f64) {
    let Some(p) = &cfg.priority else { return };
    let Some(home) = accounts.iter().find(|a| a.id == p.account_id) else { return };
    let Some(u) = home.usage.filter(|_| home.is_active && home.is_fresh) else { return };
    let baseline = planner::priority_baseline(&u, now);
    if p.baseline != Some(baseline) {
        cfg.priority = Some(Priority { account_id: home.id.clone(), baseline: Some(baseline) });
    }
}

fn is_active_login_error(e: &SwitchError) -> bool {
    matches!(e, SwitchError::ActiveAccountNotInDashboard(_) | SwitchError::ActiveAccountUnknown)
}

/// One planner account per row (macOS `autoSwitchAccounts`): display name is the email, else
/// the name; fresh means the last fetch succeeded; critical is assessed only for a fresh,
/// active account.
pub fn accounts_from(
    rows: &[DisplayRow],
    availability: impl Fn(&str) -> Option<Availability>,
    params: &CriticalParameters,
    now: f64,
) -> Vec<Account> {
    rows.iter()
        .map(|r| {
            // macOS keeps `error = "expired"` on an expired account: its numbers are stale.
            let is_fresh = r.error.is_none() && r.status != AccountStatus::Expired;
            let is_usable = r.status == AccountStatus::Active;
            let critical = if is_fresh && is_usable {
                claude_dashboard_core::critical::assess(r.seven_day.as_ref(), params, now)
            } else {
                CriticalAssessment::NONE
            };
            Account {
                id: r.account_id.clone(),
                display_name: r.email.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| r.name.clone()),
                usage: usage_of(r),
                availability: availability(&r.account_id),
                is_usable,
                is_active: r.is_active_claude_code,
                is_fresh,
                critical,
                five_hour_log: Vec::new(),
            }
        })
        .collect()
}

/// A row's 5h and 7d windows; `None` unless both are known.
pub fn usage_of(r: &DisplayRow) -> Option<Usage> {
    let w = |v: &claude_dashboard_core::rows::WindowView| Window { utilization: v.utilization, resets_at: v.resets_at_unix };
    Some(Usage { five_hour: w(r.five_hour.as_ref()?), seven_day: w(r.seven_day.as_ref()?) })
}

/// The newest 20 5h log rows, oldest first, for the near-reset rule.
pub fn log_points(series: &[SeriesPoint]) -> Vec<planner::LogPoint> {
    let start = series.len().saturating_sub(20);
    series[start..]
        .iter()
        .map(|p| planner::LogPoint { t: p.t_unix, utilization: p.utilization, resets_at: p.resets_at_unix })
        .collect()
}

/// What a card shows for auto switch: the bolt (ticked, or tickable) and the flame.
#[derive(Debug, Clone, PartialEq)]
pub struct CardFields {
    pub is_home: bool,
    /// A tick needs the active account or one auto switch could move to, with usage loaded
    /// (macOS `canTickPriority`).
    pub can_tick: bool,
    /// 0 none, 1 critical, 2 high critical.
    pub critical_level: i32,
    pub critical_help: String,
}

pub fn card_fields(
    row: &DisplayRow,
    home_id: Option<&str>,
    availability: Option<Availability>,
    params: &CriticalParameters,
    now: f64,
) -> CardFields {
    let assessment = if row.error.is_none() && row.status == AccountStatus::Active {
        claude_dashboard_core::critical::assess(row.seven_day.as_ref(), params, now)
    } else {
        CriticalAssessment::NONE
    };
    CardFields {
        is_home: home_id == Some(row.account_id.as_str()),
        can_tick: (availability == Some(Availability::Ready) || row.is_active_claude_code) && usage_of(row).is_some(),
        critical_level: match assessment.level {
            CriticalLevel::None => 0,
            CriticalLevel::Critical => 1,
            CriticalLevel::High => 2,
        },
        critical_help: claude_dashboard_core::critical::badge(&assessment, params).unwrap_or_default(),
    }
}

/// The bolt's click (macOS `togglePriority`): unticks the home account, or ticks `row`
/// (moving the tick off any other account) with its baseline: the window Claude Code uses
/// when it is active, else its current 5h reset.
pub fn toggled_priority(current: Option<&Priority>, row: &DisplayRow, now: f64) -> Option<Priority> {
    if current.is_some_and(|p| p.account_id == row.account_id) {
        return None;
    }
    let usage = usage_of(row);
    let baseline = if row.is_active_claude_code {
        usage.map(|u| planner::priority_baseline(&u, now))
    } else {
        usage.and_then(|u| u.five_hour.resets_at)
    };
    Some(Priority { account_id: row.account_id.clone(), baseline })
}

/// The flyout footer's right half (macOS `PopoverFooter`): shown while auto switch is on,
/// or near a limit when the menu bar toggle is allowed; dimmed outside the toggle zone.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Footer {
    pub shown: bool,
    pub on: bool,
    pub near: bool,
    /// "{event} · {time}", elided in the middle (macOS `.truncationMode(.middle)`); empty
    /// without an event.
    pub event: String,
    /// The event's tooltip: its text alone (macOS `.help(event.text)`).
    pub tip: String,
    pub failed: bool,
}

/// The footer event's character budget: about what fits between the version and the toggle in
/// the flyout's right half at 11 px.
pub const FOOTER_EVENT_CHARS: usize = 12;

/// `s` cut to `max` characters with an ellipsis in the middle.
pub fn elide_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max || max < 3 {
        return s.to_string();
    }
    let head = max / 2 + 1;
    let tail = max - 1 - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('\u{2026}');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// Writes the evaluated home tick back, unless the stored one changed since `before` was read
/// (a bolt click while a switch ran): the user's tick wins.
pub fn write_back_priority(s: &mut Settings, before: &Option<Priority>, after: &Option<Priority>) {
    if Config::from_settings(s).priority != *before {
        return;
    }
    s.auto_switch_priority_id = after.as_ref().map(|p| p.account_id.clone());
    s.auto_switch_priority_baseline = after.as_ref().and_then(|p| p.baseline);
}

pub fn footer(settings: &Settings, rows: &[DisplayRow], event: Option<&Event>, local_offset_s: f64) -> Footer {
    let thresholds = thresholds_of(settings);
    let near = rows
        .iter()
        .find(|r| r.is_active_claude_code)
        .and_then(usage_of)
        .is_some_and(|u| planner::is_in_toggle_zone(&u, &thresholds));
    let on = settings.auto_switch_enabled;
    Footer {
        shown: on || (settings.auto_switch_show_widget_toggle && near),
        on,
        near,
        event: event
            .map(|e| elide_middle(&format!("{} \u{00B7} {}", e.text(), crate::ping::clock(e.date_unix, local_offset_s)), FOOTER_EVENT_CHARS))
            .unwrap_or_default(),
        tip: event.map(Event::text).unwrap_or_default(),
        failed: event.is_some_and(Event::is_failure),
    }
}

fn percent(v: f64) -> String {
    format!("{}%", v.round() as i64)
}

/// `"5h 12%, 7d 30%"`.
pub fn usage_text(a: &Account) -> String {
    format!("5h {}, 7d {}", percent(a.five_hour()), percent(a.seven_day()))
}

/// The reason a failed switch gives (macOS `describe`).
pub fn describe(e: &SwitchError) -> String {
    match e {
        SwitchError::NotCaptured | SwitchError::LoginExpired => "it needs a browser sign-in first".into(),
        SwitchError::ActiveAccountNotInDashboard(_) | SwitchError::ActiveAccountUnknown => {
            "Claude Code's current login is not a dashboard account".into()
        }
        other => other.log_name().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1_800_000_000.0;
    const H: f64 = 3600.0;
    const FIVE_H: f64 = 5.0 * 3600.0;

    fn account(name: &str, five: f64, seven: f64, five_resets: Option<f64>, active: bool, now: f64) -> Account {
        Account {
            id: name.into(),
            display_name: format!("{name}@x.com"),
            usage: Some(Usage {
                five_hour: Window { utilization: five, resets_at: five_resets },
                seven_day: Window { utilization: seven, resets_at: Some(now + 86_400.0) },
            }),
            availability: Some(if active { Availability::Active } else { Availability::Ready }),
            is_usable: true,
            is_active: active,
            is_fresh: true,
            critical: CriticalAssessment::NONE,
            five_hour_log: Vec::new(),
        }
    }

    fn crit(mut a: Account, level: CriticalLevel) -> Account {
        a.critical = CriticalAssessment { level, waste: 10.0 };
        a
    }

    fn cfg() -> Config {
        Config { enabled: true, thresholds: Thresholds { five_hour: 95.0, seven_day: 99.0 }, priority: None }
    }

    struct Run {
        result: Option<String>,
        performed: Vec<String>,
        logged: Vec<NewEntry>,
    }

    fn run(c: &mut Controller, cfg: &mut Config, accounts: &[Account], now: f64, outcome: Result<(), SwitchError>) -> Run {
        let mut logged = Vec::new();
        let mut performed = Vec::new();
        let result = c.evaluate(cfg, accounts, now, |_| 0, &mut |e| logged.push(e), |id| {
            performed.push(id.to_string());
            outcome
        });
        Run { result, performed, logged }
    }

    #[test]
    fn disabled_does_nothing() {
        let mut c = Controller::default();
        let mut cfg = Config { enabled: false, ..cfg() };
        let r = run(&mut c, &mut cfg, &[account("a", 96.0, 10.0, None, true, T0), account("b", 0.0, 10.0, None, false, T0)], T0, Ok(()));
        assert_eq!((r.result, r.performed.len(), r.logged.len()), (None, 0, 0));
    }

    #[test]
    fn switch_sets_lock_event_and_log_row() {
        let mut c = Controller::default();
        let accounts = [account("a", 96.0, 10.0, None, true, T0), account("b", 10.0, 100.0, None, false, T0)];
        // b at 7d 100 is no candidate; use 99.5 for tier 3.
        let accounts = [accounts[0].clone(), Account { usage: Some(Usage { seven_day: Window { utilization: 99.5, resets_at: Some(T0 + 86_400.0) }, ..accounts[1].usage.unwrap() }), ..accounts[1].clone() }];
        let r = run(&mut c, &mut cfg(), &accounts, T0, Ok(()));
        assert_eq!(r.result.as_deref(), Some("b"));
        assert_eq!(r.performed, vec!["b"]);
        let lock = c.lock.clone().unwrap();
        assert_eq!((lock.account_id.as_str(), lock.five_hour, lock.seven_day.is_some()), ("b", None, true));
        let e = c.last_event.clone().unwrap();
        assert_eq!(e.kind, EventKind::Switched(Reason::Threshold(3)));
        assert_eq!(e.text(), "Auto-switched to b@x.com");
        assert_eq!(
            r.logged,
            vec![NewEntry {
                date_unix: T0,
                outcome: Outcome::Switched,
                from_name: Some("a@x.com".into()),
                to_name: Some("b@x.com".into()),
                reason: "5h 96% >= 95%, tier 3".into(),
                detail: Some("b@x.com: 5h 10%, 7d 100%".into()),
            }]
        );
    }

    #[test]
    fn failure_starts_cooldown_and_is_logged() {
        let mut c = Controller::default();
        let accounts = [account("a", 96.0, 99.0, None, true, T0), account("b", 0.0, 10.0, None, false, T0)];
        let r = run(&mut c, &mut cfg(), &accounts, T0, Err(SwitchError::NotCaptured));
        assert_eq!((r.result, r.performed.len()), (None, 1));
        assert!(c.last_event.as_ref().unwrap().is_failure());
        assert_eq!(c.last_event.as_ref().unwrap().text(), "Could not switch to b@x.com: it needs a browser sign-in first");
        assert_eq!(r.logged[0].outcome, Outcome::Failed);
        assert_eq!(r.logged[0].reason, "5h 96% >= 95%, 7d 99% >= 99%, tier 1");
        assert_eq!(r.logged[0].detail.as_deref(), Some("it needs a browser sign-in first"));

        let r = run(&mut c, &mut cfg(), &accounts, T0 + 60.0, Ok(()));
        assert_eq!(r.performed.len(), 0, "cooling down: not retried a minute later");
        assert_eq!(c.last_event.as_ref().unwrap().kind, EventKind::NoCandidate);
        let r = run(&mut c, &mut cfg(), &accounts, T0 + FAILURE_COOLDOWN_S + 1.0, Ok(()));
        assert_eq!(r.performed, vec!["b"]);
    }

    #[test]
    fn active_login_failure_pauses_auto_switch() {
        let mut c = Controller::default();
        let accounts = [account("a", 96.0, 10.0, None, true, T0), account("b", 0.0, 10.0, None, false, T0), account("c", 0.0, 10.0, None, false, T0)];
        let r = run(&mut c, &mut cfg(), &accounts, T0, Err(SwitchError::ActiveAccountUnknown));
        assert_eq!(r.performed, vec!["b"]);
        assert_eq!(r.logged[0].detail.as_deref(), Some("Claude Code's current login is not a dashboard account"));
        let r = run(&mut c, &mut cfg(), &accounts, T0 + 60.0, Ok(()));
        assert_eq!((r.performed.len(), r.logged.len()), (0, 0), "paused: the current login is the problem");
        let r = run(&mut c, &mut cfg(), &accounts, T0 + FAILURE_COOLDOWN_S + 1.0, Ok(()));
        assert_eq!(r.performed, vec!["b"], "the pause ends, and b was never put on cooldown");
    }

    #[test]
    fn no_candidate_is_logged_once_per_stretch() {
        let mut c = Controller::default();
        let stuck = [account("a", 96.0, 10.0, None, true, T0)];
        let r1 = run(&mut c, &mut cfg(), &stuck, T0, Ok(()));
        let r2 = run(&mut c, &mut cfg(), &stuck, T0 + 60.0, Ok(()));
        assert_eq!(c.last_event.as_ref().unwrap().text(), "No account to switch to");
        assert_eq!((r1.logged.len(), r2.logged.len()), (1, 0));
        assert_eq!(r1.logged[0].reason, "5h 96% >= 95%");
        assert_eq!((r1.logged[0].outcome, r1.logged[0].to_name.clone()), (Outcome::NoCandidate, None));
        run(&mut c, &mut cfg(), &[account("a", 50.0, 10.0, None, true, T0)], T0 + 120.0, Ok(()));
        let r3 = run(&mut c, &mut cfg(), &stuck, T0 + 180.0, Ok(()));
        assert_eq!(r3.logged.len(), 1, "a new stretch is logged");
    }

    #[test]
    fn nothing_is_logged_below_threshold() {
        let mut c = Controller::default();
        let r = run(&mut c, &mut cfg(), &[account("a", 50.0, 10.0, None, true, T0), account("b", 0.0, 10.0, None, false, T0)], T0, Ok(()));
        assert_eq!((r.result, r.logged.len(), c.last_event.clone()), (None, 0, None));
    }

    #[test]
    fn priority_switch_text_and_reason() {
        let mut c = Controller::default();
        let mut cfg = Config { priority: Some(Priority { account_id: "t".into(), baseline: Some(T0 - 600.0) }), ..cfg() };
        let r = run(&mut c, &mut cfg, &[account("a", 10.0, 10.0, None, true, T0), account("t", 0.0, 40.0, None, false, T0)], T0, Ok(()));
        assert_eq!(r.result.as_deref(), Some("t"));
        assert_eq!(c.last_event.as_ref().unwrap().text(), "Switched to t@x.com after its 5h reset");
        assert_eq!(r.logged[0].reason, "Ticked account's 5h window reset");
        assert_eq!(cfg.priority.as_ref().unwrap().account_id, "t", "the tick stays");
    }

    #[test]
    fn critical_switch_text_and_reason() {
        let mut c = Controller::default();
        let mut target = crit(account("b", 0.0, 30.0, None, false, T0), CriticalLevel::High);
        target.critical.waste = 12.4;
        let r = run(&mut c, &mut cfg(), &[account("a", 10.0, 10.0, None, true, T0), target], T0, Ok(()));
        assert_eq!(r.result.as_deref(), Some("b"));
        assert_eq!(c.last_event.as_ref().unwrap().text(), "Switched to b@x.com: high critical account");
        assert_eq!(r.logged[0].reason, "High critical account's 5h window is free, 12% of 7d would go unused");
        let mut c = Controller::default();
        let r = run(&mut c, &mut cfg(), &[account("a", 10.0, 10.0, None, true, T0), crit(account("b", 0.0, 30.0, None, false, T0), CriticalLevel::Critical)], T0, Ok(()));
        assert_eq!(c.last_event.as_ref().unwrap().text(), "Switched to b@x.com: critical account");
        assert_eq!(r.logged[0].reason, "Critical account's 5h window is free, 10% of 7d would go unused");
    }

    #[test]
    fn priority_follows_the_active_home_window_and_is_dropped_when_gone() {
        let mut c = Controller::default();
        let disabled = |p: Option<Priority>| Config { enabled: false, priority: p, ..cfg() };
        let mut cfg = disabled(Some(Priority { account_id: "h".into(), baseline: None }));
        run(&mut c, &mut cfg, &[account("h", 10.0, 10.0, Some(T0 + H), true, T0)], T0, Ok(()));
        assert_eq!(cfg.priority, Some(Priority { account_id: "h".into(), baseline: Some(T0 + H) }), "even while disabled");
        let mut cfg = disabled(Some(Priority { account_id: "h".into(), baseline: None }));
        run(&mut c, &mut cfg, &[account("h", 0.0, 10.0, None, true, T0)], T0, Ok(()));
        assert_eq!(cfg.priority.unwrap().baseline, Some(T0 + FIVE_H), "before its window starts: five hours");
        let mut stale = account("h", 10.0, 10.0, Some(T0 + H), true, T0);
        stale.is_fresh = false;
        let mut cfg = disabled(Some(Priority { account_id: "h".into(), baseline: Some(T0 - 9000.0) }));
        run(&mut c, &mut cfg, &[stale], T0, Ok(()));
        assert_eq!(cfg.priority.unwrap().baseline, Some(T0 - 9000.0), "stale usage keeps the baseline");
        let mut cfg = disabled(Some(Priority { account_id: "gone".into(), baseline: None }));
        drop_deleted_home(&mut cfg, &["a".to_string()]);
        assert_eq!(cfg.priority, None);
    }

    #[test]
    fn leaves_home_at_its_limit_and_comes_back_after_its_reset() {
        let mut c = Controller::default();
        let resets = T0 + 1800.0;
        let mut cfg = Config { priority: Some(Priority { account_id: "h".into(), baseline: None }), ..cfg() };
        let other = |active| account("o", 10.0, 20.0, Some(T0 + H), active, T0);
        let r = run(&mut c, &mut cfg, &[account("h", 96.0, 20.0, Some(resets), true, T0), other(false)], T0, Ok(()));
        assert_eq!(r.result.as_deref(), Some("o"));
        assert_eq!(cfg.priority, Some(Priority { account_id: "h".into(), baseline: Some(resets) }));
        let r = run(&mut c, &mut cfg, &[account("h", 96.0, 20.0, Some(resets), false, T0), other(true)], T0, Ok(()));
        assert_eq!(r.result, None, "its window has not reset yet");
        let r = run(&mut c, &mut cfg, &[account("h", 0.0, 20.0, None, false, T0), other(true)], resets + 60.0, Ok(()));
        assert_eq!(r.result.as_deref(), Some("h"));
    }

    #[test]
    fn fast_poll_follows_enabled_zone_and_freshness() {
        let c = Controller::default();
        let hot = [account("a", 93.0, 10.0, None, true, T0)];
        assert!(c.should_fast_poll(&cfg(), &hot));
        assert!(!c.should_fast_poll(&Config { enabled: false, ..cfg() }, &hot));
        assert!(!c.should_fast_poll(&cfg(), &[account("a", 50.0, 10.0, None, true, T0)]));
        let mut stale = hot[0].clone();
        stale.is_fresh = false;
        assert!(!c.should_fast_poll(&cfg(), &[stale]));
    }

    #[test]
    fn critical_switch_holds_until_the_targets_5h_reset() {
        let mut c = Controller::default();
        let a = account("a", 10.0, 10.0, None, true, T0);
        let b = crit(account("b", 0.0, 30.0, None, false, T0), CriticalLevel::Critical);
        let r = run(&mut c, &mut cfg(), &[a, b, account("c", 0.0, 20.0, None, false, T0)], T0, Ok(()));
        assert_eq!(r.result.as_deref(), Some("b"));
        assert_eq!(c.hold, Some(Hold { account_id: "b".into(), until: T0 + FIVE_H }));

        let now = T0 + H;
        let resets = now + 4.0 * H;
        let accounts = [
            account("a", 10.0, 10.0, None, false, now),
            account("b", 60.0, 45.0, Some(resets), true, now),
            crit(account("c", 0.0, 20.0, None, false, now), CriticalLevel::Critical),
        ];
        let r = run(&mut c, &mut cfg(), &accounts, now, Ok(()));
        assert_eq!((r.result, r.performed.len()), (None, 0));
        assert_eq!(c.hold.as_ref().unwrap().until, resets, "the hold follows b's real 5h reset");
        let r = run(&mut c, &mut cfg(), &accounts, resets, Ok(()));
        assert_eq!(r.result.as_deref(), Some("c"));
    }

    #[test]
    fn manual_switch_clears_lock_and_holds() {
        let mut c = Controller::default();
        let a = account("a", 10.0, 10.0, None, true, T0);
        let cc = crit(account("c", 0.0, 20.0, None, false, T0), CriticalLevel::Critical);
        c.lock = Some(Lock { account_id: "a".into(), five_hour: Some(None), seven_day: None });
        c.note_manual_switch("a", a.usage, T0);
        assert_eq!((c.lock.clone(), c.hold.clone()), (None, Some(Hold { account_id: "a".into(), until: T0 + FIVE_H })));
        let r = run(&mut c, &mut cfg(), &[a.clone(), cc.clone()], T0, Ok(()));
        assert_eq!((r.result, r.performed.len()), (None, 0));

        let resets = T0 + 3.0 * H;
        c.note_manual_switch("a", account("a", 50.0, 10.0, Some(resets), true, T0).usage, T0);
        assert_eq!(c.hold.as_ref().unwrap().until, resets);
        c.note_manual_switch("a", account("a", 50.0, 10.0, Some(T0 - 1.0), true, T0).usage, T0);
        assert_eq!(c.hold.as_ref().unwrap().until, T0 + FIVE_H, "a reset already past counts as not started");
    }

    #[test]
    fn hold_dropped_when_active_changes_kept_when_unknown_and_cleared_by_threshold_switch() {
        let mut c = Controller::default();
        let a = account("a", 10.0, 10.0, None, true, T0);
        let cc = crit(account("c", 0.0, 20.0, None, false, T0), CriticalLevel::Critical);
        c.note_manual_switch("a", a.usage, T0);
        let held = c.hold.clone();
        run(&mut c, &mut cfg(), &[account("a", 10.0, 10.0, None, false, T0), cc.clone()], T0, Ok(()));
        assert_eq!(c.hold, held, "no active account detected: kept");
        let r = run(&mut c, &mut cfg(), &[account("a", 10.0, 10.0, None, false, T0), account("b", 10.0, 10.0, None, true, T0), cc], T0, Ok(()));
        assert_eq!(r.result.as_deref(), Some("c"));
        assert_eq!(c.hold.as_ref().unwrap().account_id, "c");

        let mut c = Controller::default();
        c.note_manual_switch("a", None, T0);
        let r = run(&mut c, &mut cfg(), &[account("a", 96.0, 10.0, None, true, T0), account("b", 0.0, 10.0, None, false, T0)], T0, Ok(()));
        assert_eq!((r.result.as_deref(), c.hold.clone()), (Some("b"), None));
    }

    fn row(id: &str, email: Option<&str>, five: Option<f64>, seven: Option<(f64, f64)>) -> DisplayRow {
        use claude_dashboard_core::model::{AccountPlan, AccountStatus};
        use claude_dashboard_core::rows::WindowView;
        DisplayRow {
            account_id: id.into(),
            name: format!("{id}-name"),
            email: email.map(Into::into),
            plan: AccountPlan::Pro,
            status: AccountStatus::Active,
            five_hour: five.map(|u| WindowView { utilization: u, resets_at_unix: Some(T0 + H), is_limited: false }),
            seven_day: seven.map(|(u, r)| WindowView { utilization: u, resets_at_unix: Some(r), is_limited: false }),
            fable: None,
            peak_utilization: 0.0,
            burn_projected_seconds: None,
            burn_projected_seconds_7d: None,
            burn_projected_seconds_fable: None,
            is_extension_sourced: false,
            error: None,
            last_synced_unix: None,
            is_active_claude_code: false,
            is_pinned: false,
            kind_label: String::new(),
            expired_hint: String::new(),
        }
    }

    #[test]
    fn accounts_from_rows() {
        use claude_dashboard_core::model::AccountStatus;
        let params = CriticalParameters { x: 16.0, pace: 50.0, high_pace: 80.0, quiet: None };
        let critical_week = Some((55.0, T0 + 25.0 * H));
        let mut active = row("a", Some("a@x.com"), Some(40.0), critical_week);
        active.is_active_claude_code = true;
        let mut stale = row("s", None, Some(10.0), critical_week);
        stale.error = Some("boom".into());
        let mut expired = row("e", None, Some(10.0), critical_week);
        expired.status = AccountStatus::Expired;
        let half = row("h", None, Some(10.0), None);
        let avail = |id: &str| match id {
            "a" => Some(Availability::Active),
            "s" => Some(Availability::Ready),
            _ => None,
        };
        let out = accounts_from(&[active, stale, expired, half], avail, &params, T0);
        assert_eq!(out.iter().map(|a| a.display_name.as_str()).collect::<Vec<_>>(), vec!["a@x.com", "s-name", "e-name", "h-name"]);
        assert!(out[0].is_active && out[0].is_fresh && out[0].is_usable);
        assert_eq!(out[0].availability, Some(Availability::Active));
        assert_eq!(out[0].critical.level, CriticalLevel::Critical);
        assert_eq!(out[0].usage.unwrap().five_hour, Window { utilization: 40.0, resets_at: Some(T0 + H) });
        assert!(!out[1].is_fresh);
        assert_eq!(out[1].critical, CriticalAssessment::NONE, "stale: not assessed");
        assert!(!out[2].is_usable);
        assert_eq!(out[2].critical, CriticalAssessment::NONE, "not active: not assessed");
        assert_eq!(out[3].usage, None, "a missing window means no usage");
    }

    #[test]
    fn log_points_keep_the_newest_twenty_oldest_first() {
        let series: Vec<SeriesPoint> = (0..25)
            .map(|i| SeriesPoint { t_unix: T0 + i as f64, utilization: i as f64, resets_at_unix: T0 + H, is_limited: false })
            .collect();
        let points = log_points(&series);
        assert_eq!(points.len(), 20);
        assert_eq!((points[0].utilization, points[19].utilization), (5.0, 24.0));
        assert_eq!(points[0].resets_at, T0 + H);
    }

    #[test]
    fn card_fields_for_bolt_and_flame() {
        let params = CriticalParameters { x: 16.0, pace: 50.0, high_pace: 80.0, quiet: None };
        let critical = row("a", None, Some(10.0), Some((55.0, T0 + 25.0 * H)));
        let f = card_fields(&critical, Some("a"), Some(Availability::Ready), &params, T0);
        assert!(f.is_home && f.can_tick);
        assert_eq!(f.critical_level, 1);
        assert_eq!(f.critical_help, "Critical: at 50% of 5h per window around the clock, 7d reaches 95% by its reset (5% unused)");
        let high = row("b", None, Some(10.0), Some((60.0, T0 + 10.0 * H)));
        assert_eq!(card_fields(&high, None, Some(Availability::Ready), &params, T0).critical_level, 2);
        let calm = row("c", None, Some(10.0), Some((10.0, T0 + 6.0 * 86_400.0)));
        let f = card_fields(&calm, Some("a"), Some(Availability::NotCaptured), &params, T0);
        assert_eq!((f.is_home, f.can_tick, f.critical_level, f.critical_help.as_str()), (false, false, 0, ""));
        let mut active = calm.clone();
        active.is_active_claude_code = true;
        assert!(card_fields(&active, None, Some(Availability::Active), &params, T0).can_tick, "the active account can be ticked");
        let no_usage = row("d", None, None, None);
        assert!(!card_fields(&no_usage, None, Some(Availability::Ready), &params, T0).can_tick, "needs usage for the baseline");
        let mut stale = critical.clone();
        stale.error = Some("x".into());
        assert_eq!(card_fields(&stale, None, None, &params, T0).critical_level, 0, "stale: no flame");
    }

    #[test]
    fn toggled_priority_ticks_moves_and_unticks() {
        let mut r = row("a", None, Some(30.0), Some((10.0, T0 + 86_400.0)));
        assert_eq!(toggled_priority(None, &r, T0), Some(Priority { account_id: "a".into(), baseline: Some(T0 + H) }));
        let other = Priority { account_id: "z".into(), baseline: None };
        assert_eq!(toggled_priority(Some(&other), &r, T0).unwrap().account_id, "a", "the tick moves");
        let mine = Priority { account_id: "a".into(), baseline: Some(1.0) };
        assert_eq!(toggled_priority(Some(&mine), &r, T0), None, "a second click unticks");
        r.is_active_claude_code = true;
        r.five_hour.as_mut().unwrap().resets_at_unix = None;
        assert_eq!(toggled_priority(None, &r, T0).unwrap().baseline, Some(T0 + FIVE_H), "active, not started: five hours");
        r.is_active_claude_code = false;
        assert_eq!(toggled_priority(None, &r, T0).unwrap().baseline, None, "not active, not started: no baseline");
    }

    #[test]
    fn footer_visibility_dimming_and_event() {
        let mut s = Settings::default();
        let mut hot = row("a", Some("a@x.com"), Some(80.0), Some((10.0, T0 + 86_400.0)));
        hot.is_active_claude_code = true;
        let mut calm = hot.clone();
        calm.five_hour.as_mut().unwrap().utilization = 79.0;
        let f = footer(&s, &[calm.clone()], None, 0.0);
        assert_eq!(f, Footer { shown: true, on: true, near: false, event: String::new(), tip: String::new(), failed: false }, "on: shown, dimmed");
        s.auto_switch_enabled = false;
        assert!(!footer(&s, &[calm.clone()], None, 0.0).shown, "off and calm: hidden");
        let f = footer(&s, &[hot.clone()], None, 0.0);
        assert!(f.shown && !f.on && f.near, "off but near a limit: shown so it can be turned on");
        s.auto_switch_show_widget_toggle = false;
        assert!(!footer(&s, &[hot.clone()], None, 0.0).shown, "the menu bar toggle setting is off");
        let s = Settings::default();
        let e = Event { kind: EventKind::Failed("x".into()), account_name: Some("b@x.com".into()), date_unix: 1_000_000.0 };
        let f = footer(&s, &[hot], Some(&e), 0.0);
        assert_eq!(f.event, elide_middle("Could not switch to b@x.com: x \u{00B7} 1:46 PM", FOOTER_EVENT_CHARS));
        assert_eq!(f.event.chars().count(), FOOTER_EVENT_CHARS);
        assert_eq!(f.tip, "Could not switch to b@x.com: x");
        assert!(f.failed);
        assert!(!footer(&Settings::default(), &[], None, 0.0).near, "no active account: not near");
    }

    /// macOS keeps `error = "expired"` on an expired account, so its last numbers are stale:
    /// auto switch must not act on them (review finding 2).
    #[test]
    fn an_expired_active_account_is_not_fresh() {
        use claude_dashboard_core::model::AccountStatus;
        let params = CriticalParameters { x: 16.0, pace: 50.0, high_pace: 80.0, quiet: None };
        let mut expired = row("a", None, Some(96.0), Some((10.0, T0 + 86_400.0)));
        expired.is_active_claude_code = true;
        expired.status = AccountStatus::Expired;
        let accounts = accounts_from(&[expired, row("b", None, Some(0.0), Some((10.0, T0 + 86_400.0)))], |id| {
            Some(if id == "a" { Availability::Active } else { Availability::Ready })
        }, &params, T0);
        assert!(!accounts[0].is_fresh);
        let mut c = Controller::default();
        let r = run(&mut c, &mut cfg(), &accounts, T0, Ok(()));
        assert_eq!(r.result, None, "no switch on frozen numbers");
        assert!(!c.should_fast_poll(&cfg(), &accounts), "no endless poll of an account never fetched");
    }

    /// A refresh that read no rows (the store could not be read) must not drop the tick
    /// (review finding 3): only an account gone from the store drops it.
    #[test]
    fn the_home_account_is_dropped_only_when_the_store_no_longer_has_it() {
        let mut c = Controller::default();
        let mut cfg = Config { priority: Some(Priority { account_id: "h".into(), baseline: None }), ..cfg() };
        run(&mut c, &mut cfg, &[], T0, Ok(()));
        assert!(cfg.priority.is_some(), "no rows: kept");
        drop_deleted_home(&mut cfg, &["h".to_string(), "x".to_string()]);
        assert!(cfg.priority.is_some());
        drop_deleted_home(&mut cfg, &["x".to_string()]);
        assert_eq!(cfg.priority, None);
    }

    /// The write-back changes the stored tick only when nobody changed it meanwhile (minor 4).
    #[test]
    fn priority_write_back_keeps_a_concurrent_tick() {
        let p = |id: &str, b: f64| Some(Priority { account_id: id.into(), baseline: Some(b) });
        let mut s = Settings { auto_switch_priority_id: Some("h".into()), auto_switch_priority_baseline: Some(1.0), ..Settings::default() };
        write_back_priority(&mut s, &p("h", 1.0), &p("h", 2.0));
        assert_eq!(s.auto_switch_priority_baseline, Some(2.0), "unchanged meanwhile: written");
        s.auto_switch_priority_id = Some("other".into());
        write_back_priority(&mut s, &p("h", 2.0), &p("h", 3.0));
        assert_eq!(s.auto_switch_priority_id.as_deref(), Some("other"), "the user's new tick wins");
        write_back_priority(&mut s, &p("other", 9.0), &None);
        assert_eq!(s.auto_switch_priority_id.as_deref(), Some("other"), "baseline differs: not ours to drop");
    }

    #[test]
    fn footer_event_elides_in_the_middle_and_tips_without_the_time() {
        assert_eq!(elide_middle("short", 10), "short");
        assert_eq!(elide_middle("Could not switch to bob@example.com: config", 21), "Could not s\u{2026}m: config");
        let s = Settings::default();
        let mut hot = row("a", None, Some(80.0), Some((10.0, T0 + 86_400.0)));
        hot.is_active_claude_code = true;
        let e = Event { kind: EventKind::NoCandidate, account_name: None, date_unix: 1_000_000.0 };
        let f = footer(&s, &[hot], Some(&e), 0.0);
        assert_eq!(f.tip, "No account to switch to", "macOS .help(event.text): no time");
    }

    #[test]
    fn describe_errors() {
        assert_eq!(describe(&SwitchError::LoginExpired), "it needs a browser sign-in first");
        assert_eq!(describe(&SwitchError::ActiveAccountNotInDashboard("x@y".into())), "Claude Code's current login is not a dashboard account");
        assert_eq!(describe(&SwitchError::Config), "config");
    }
}
