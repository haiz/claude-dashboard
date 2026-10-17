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
use claude_dashboard_core::settings::Settings;

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

#[derive(Debug, Default)]
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

/// Drops a tick on a removed account. While the ticked account is active, moves its
/// baseline to the window Claude Code uses, so leaving it waits for that window's reset.
fn update_priority(cfg: &mut Config, accounts: &[Account], now: f64) {
    let Some(p) = &cfg.priority else { return };
    let Some(home) = accounts.iter().find(|a| a.id == p.account_id) else {
        cfg.priority = None;
        return;
    };
    let Some(u) = home.usage.filter(|_| home.is_active && home.is_fresh) else { return };
    let baseline = planner::priority_baseline(&u, now);
    if p.baseline != Some(baseline) {
        cfg.priority = Some(Priority { account_id: home.id.clone(), baseline: Some(baseline) });
    }
}

fn is_active_login_error(e: &SwitchError) -> bool {
    matches!(e, SwitchError::ActiveAccountNotInDashboard(_) | SwitchError::ActiveAccountUnknown)
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
    use claude_dashboard_core::auto_switch::Window;
    use claude_dashboard_core::cc_switch::switcher::Availability;
    use claude_dashboard_core::critical::CriticalAssessment;

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
        run(&mut c, &mut cfg, &[account("a", 10.0, 10.0, None, true, T0)], T0, Ok(()));
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

    #[test]
    fn describe_errors() {
        assert_eq!(describe(&SwitchError::LoginExpired), "it needs a browser sign-in first");
        assert_eq!(describe(&SwitchError::ActiveAccountNotInDashboard("x@y".into())), "Claude Code's current login is not a dashboard account");
        assert_eq!(describe(&SwitchError::Config), "config");
    }
}
