//! Which account Claude Code should move to, if any. Pure port of macOS `AutoSwitchPlanner`:
//! priority order high critical, home, critical, then the threshold tiers. A hold on the
//! active account turns off the two critical rules until it ends. The thresholds wait out
//! the last minutes of the home or most critical account's 5h window (`waits_for_reset`).
//!
//! ```text
//! let d = decide(&snapshot, |n| rand_index(n));
//! if let Decision::SwitchTo(id, _) = d { ... }
//! ```

use std::collections::HashSet;

use crate::cc_switch::switcher::Availability;
use crate::critical::{CriticalAssessment, CriticalLevel};

pub const TIER_FIVE_HOUR_LIMIT: f64 = 91.0;
pub const TIER_SEVEN_DAY_LIMIT: f64 = 99.0;
pub const SCORE_SEVEN_DAY_CAP: f64 = 98.0;
pub const SCORE_MIN_DAYS_LEFT: f64 = 0.25;
pub const GROUP_RATIO: f64 = 0.85;
/// `resetsAt` drifts by sub-seconds between fetches; a real reset moves it by hours.
pub const RESET_TOLERANCE_S: f64 = 60.0;
pub const FIVE_HOUR_WINDOW_S: f64 = 5.0 * 3600.0;
/// A critical target's 5h counts as free at or under this (an Auto Ping leaves about 1%).
pub const FREE_FIVE_HOUR_LIMIT: f64 = 2.0;
/// The near-reset rule looks at the last minutes of a 5h window, and keeps this much of the
/// 5h quota unused as a margin.
pub const NEAR_RESET_WINDOW_S: f64 = 5.0 * 60.0;
pub const NEAR_RESET_MARGIN: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub utilization: f64,
    pub resets_at: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    pub five_hour: Window,
    pub seven_day: Window,
}

/// One 5h `usage_logs` row of Claude Code's account.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogPoint {
    pub t: f64,
    pub utilization: f64,
    pub resets_at: f64,
}

/// One account as auto switch sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: String,
    /// Email when known, else the account name. Shown in the footer and the log.
    pub display_name: String,
    pub usage: Option<Usage>,
    pub availability: Option<Availability>,
    /// `Account.status == Active`.
    pub is_usable: bool,
    /// Claude Code uses this account now.
    pub is_active: bool,
    /// The last fetch succeeded. A failed fetch keeps the previous usage on the card;
    /// deciding on those numbers would act on stale data.
    pub is_fresh: bool,
    pub critical: CriticalAssessment,
    /// The newest 5h log rows, oldest first. Filled for Claude Code's account only.
    pub five_hour_log: Vec<LogPoint>,
}

impl Account {
    pub fn five_hour(&self) -> f64 {
        self.usage.map_or(100.0, |u| u.five_hour.utilization)
    }

    pub fn seven_day(&self) -> f64 {
        self.usage.map_or(100.0, |u| u.seven_day.utilization)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    pub five_hour: f64,
    pub seven_day: f64,
}

/// The home account: Claude Code moves back to it each time its 5h window resets.
/// `baseline` is its 5h `resetsAt` when ticked, or when Claude Code last used it; `None`
/// means it was ticked before its window started.
#[derive(Debug, Clone, PartialEq)]
pub struct Priority {
    pub account_id: String,
    pub baseline: Option<f64>,
}

/// After an auto switch lands on an account already at its threshold in a window, that
/// window's trigger is raised to 100 until it is used up or resets. Each window keeps its
/// `resetsAt` from when it was locked (`Some(None)`: locked while not started).
#[derive(Debug, Clone, PartialEq)]
pub struct Lock {
    pub account_id: String,
    pub five_hour: Option<Option<f64>>,
    pub seven_day: Option<Option<f64>>,
}

/// After a critical switch or a manual Switch, Claude Code stays on that account until
/// `until` (its 5h reset): the critical rules do not move it while the hold lasts.
#[derive(Debug, Clone, PartialEq)]
pub struct Hold {
    pub account_id: String,
    pub until: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub accounts: Vec<Account>,
    pub thresholds: Thresholds,
    pub lock: Option<Lock>,
    pub hold: Option<Hold>,
    pub priority: Option<Priority>,
    /// Accounts in the failure cooldown.
    pub excluded: HashSet<String>,
    pub now: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    PriorityReset,
    Critical(CriticalLevel),
    Threshold(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StayReason {
    /// Claude Code's account is not on the dashboard, or its usage never loaded or is stale.
    NoActiveAccount,
    BelowThreshold,
    NoCandidate,
    /// Over the 5h threshold, but the 5h window resets within minutes at a pace that fits.
    NearReset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Stay(StayReason),
    SwitchTo(String, Reason),
}

pub fn is_triggered(u: &Usage, t: &Thresholds) -> bool {
    u.five_hour.utilization >= t.five_hour || u.seven_day.utilization >= t.seven_day
}

pub fn is_in_warning_zone(u: &Usage, t: &Thresholds) -> bool {
    u.five_hour.utilization >= t.five_hour - 3.0 || u.seven_day.utilization >= t.seven_day - 1.0
}

/// Wider than the warning zone: the footer toggle appears early enough to turn auto switch
/// on before the fast poll would start.
pub fn is_in_toggle_zone(u: &Usage, t: &Thresholds) -> bool {
    u.five_hour.utilization >= t.five_hour - 15.0 || u.seven_day.utilization >= t.seven_day - 3.0
}

/// The 7d percent left below 98 per day left in the week. Days left are floored at 0.25 and
/// capped at 7. A week that has not started scores 0, keeping the account in reserve.
pub fn score(u: &Usage, now: f64) -> f64 {
    let Some(resets) = u.seven_day.resets_at else { return 0.0 };
    let days_left = ((resets - now) / 86_400.0).clamp(0.0, 7.0);
    (SCORE_SEVEN_DAY_CAP - u.seven_day.utilization).max(0.0) / days_left.max(SCORE_MIN_DAYS_LEFT)
}

/// True when a window that had `baseline` as its `resetsAt` has since reset.
pub fn has_reset(current: Option<f64>, baseline: Option<f64>) -> bool {
    match (current, baseline) {
        (Some(c), Some(b)) => c - b > RESET_TOLERANCE_S,
        _ => true,
    }
}

/// The ticked account's baseline while Claude Code uses it: its 5h `resetsAt`, or the
/// earliest time a window started now could reset.
pub fn priority_baseline(u: &Usage, now: f64) -> f64 {
    u.five_hour.resets_at.unwrap_or(now + FIVE_HOUR_WINDOW_S)
}

/// True when the ticked account's 5h window has reset since `baseline`. A `None` current
/// counts only once the baseline has passed.
pub fn is_priority_due(current: Option<f64>, baseline: Option<f64>, now: f64) -> bool {
    if current.is_none() && baseline.is_some_and(|b| b > now) {
        return false;
    }
    has_reset(current, baseline)
}

pub fn effective_thresholds(id: &str, base: &Thresholds, lock: Option<&Lock>) -> Thresholds {
    match lock {
        Some(l) if l.account_id == id => Thresholds {
            five_hour: if l.five_hour.is_some() { 100.0 } else { base.five_hour },
            seven_day: if l.seven_day.is_some() { 100.0 } else { base.seven_day },
        },
        _ => *base,
    }
}

/// The lock for an account just switched to: the windows already at their threshold.
pub fn make_lock(id: &str, u: &Usage, t: &Thresholds) -> Option<Lock> {
    let five = (u.five_hour.utilization >= t.five_hour).then_some(u.five_hour.resets_at);
    let seven = (u.seven_day.utilization >= t.seven_day).then_some(u.seven_day.resets_at);
    (five.is_some() || seven.is_some()).then(|| Lock { account_id: id.to_string(), five_hour: five, seven_day: seven })
}

/// Drops windows that are used up (100) or have reset, and the whole lock once Claude Code
/// uses another account.
pub fn updated_lock(lock: Option<Lock>, active: Option<&Account>) -> Option<Lock> {
    let mut lock = lock?;
    let active = active?;
    if active.id != lock.account_id {
        return None;
    }
    let Some(u) = active.usage else { return Some(lock) };
    if lock.five_hour.is_some_and(|w| u.five_hour.utilization >= 100.0 || has_reset(u.five_hour.resets_at, w)) {
        lock.five_hour = None;
    }
    if lock.seven_day.is_some_and(|w| u.seven_day.utilization >= 100.0 || has_reset(u.seven_day.resets_at, w)) {
        lock.seven_day = None;
    }
    (lock.five_hour.is_some() || lock.seven_day.is_some()).then_some(lock)
}

pub fn is_candidate(a: &Account, excluded: &HashSet<String>) -> bool {
    !a.is_active
        && a.availability == Some(Availability::Ready)
        && a.is_usable
        && a.is_fresh
        && a.usage.is_some()
        && !excluded.contains(&a.id)
        && a.five_hour() < 100.0
        && a.seven_day() < 100.0
}

/// A critical candidate whose 5h window is free and whose week is not used up.
pub fn is_critical_target(a: &Account, excluded: &HashSet<String>) -> bool {
    if a.critical.level == CriticalLevel::None || !is_candidate(a, excluded) || a.seven_day() >= TIER_SEVEN_DAY_LIMIT {
        return false;
    }
    a.usage.is_some_and(|u| u.five_hour.resets_at.is_none() || u.five_hour.utilization <= FREE_FIVE_HOUR_LIMIT)
}

/// True when Claude Code should sit out the last minutes of its 5h window instead of a
/// threshold switch it would undo at the reset.
pub fn waits_for_reset(active: &Account, limits: &Thresholds, targets: &[&Account], s: &Snapshot) -> bool {
    let others: Vec<&&Account> = targets.iter().filter(|t| t.id != active.id).collect();
    let is_home = s.priority.as_ref().is_some_and(|p| p.account_id == active.id);
    let qualifies = if is_home {
        !others.iter().any(|t| t.critical.level == CriticalLevel::High)
    } else {
        active.critical.level != CriticalLevel::None && !others.iter().any(|t| t.critical.waste > active.critical.waste)
    };
    let Some(u) = active.usage else { return false };
    let Some(resets) = u.five_hour.resets_at else { return false };
    if !qualifies || u.seven_day.utilization >= limits.seven_day {
        return false;
    }
    let left = resets - s.now;
    if left <= 0.0 || left > NEAR_RESET_WINDOW_S {
        return false;
    }
    let cutoff = s.now - left - 60.0;
    // The newest row; the first of equal times, as Swift's `max(by:)` keeps.
    let earlier = active
        .five_hour_log
        .iter()
        .filter(|r| r.t <= cutoff && (r.resets_at - resets).abs() <= RESET_TOLERANCE_S)
        .fold(None, |best: Option<&LogPoint>, r| match best {
            Some(b) if r.t <= b.t => Some(b),
            _ => Some(r),
        });
    let Some(earlier) = earlier else { return false };
    let used = u.five_hour.utilization - earlier.utilization;
    used <= 100.0 - u.five_hour.utilization - NEAR_RESET_MARGIN
}

/// The target with the largest waste; the first of equal maxima, as Swift's `max(by:)` keeps.
fn most_wasteful<'a>(list: impl Iterator<Item = &'a Account>) -> Option<&'a Account> {
    list.fold(None, |best: Option<&Account>, a| match best {
        Some(b) if a.critical.waste <= b.critical.waste => Some(b),
        _ => Some(a),
    })
}

/// `pick_index(n)` returns an index in `0..n`; production passes a random pick.
pub fn decide(s: &Snapshot, mut pick_index: impl FnMut(usize) -> usize) -> Decision {
    let Some(active) = s.accounts.iter().find(|a| a.is_active) else {
        return Decision::Stay(StayReason::NoActiveAccount);
    };
    let Some(active_usage) = active.usage.filter(|_| active.is_fresh) else {
        return Decision::Stay(StayReason::NoActiveAccount);
    };
    let candidates: Vec<&Account> = s.accounts.iter().filter(|a| is_candidate(a, &s.excluded)).collect();
    let targets: Vec<&Account> = s.accounts.iter().filter(|a| is_critical_target(a, &s.excluded)).collect();
    let active_is_high = active.critical.level == CriticalLevel::High;
    let is_held = s.hold.as_ref().is_some_and(|h| h.account_id == active.id && s.now < h.until);

    // 1. High critical beats everything, home included, unless Claude Code is held.
    if !active_is_high && !is_held {
        if let Some(t) = most_wasteful(targets.iter().copied().filter(|t| t.critical.level == CriticalLevel::High)) {
            return Decision::SwitchTo(t.id.clone(), Reason::Critical(CriticalLevel::High));
        }
    }

    // 2. Home, unless Claude Code is on a high critical account.
    if !active_is_high {
        if let Some(p) = &s.priority {
            if let Some(target) = candidates.iter().find(|a| a.id == p.account_id) {
                let current = target.usage.and_then(|u| u.five_hour.resets_at);
                if target.seven_day() < TIER_SEVEN_DAY_LIMIT && is_priority_due(current, p.baseline, s.now) {
                    return Decision::SwitchTo(target.id.clone(), Reason::PriorityReset);
                }
            }
        }
    }

    // 3. Critical beats an ordinary account that is not home, unless Claude Code is held.
    let active_is_home = s.priority.as_ref().is_some_and(|p| p.account_id == active.id);
    if !is_held && active.critical.level == CriticalLevel::None && !active_is_home {
        if let Some(t) = most_wasteful(targets.iter().copied()) {
            return Decision::SwitchTo(t.id.clone(), Reason::Critical(t.critical.level));
        }
    }

    let limits = effective_thresholds(&active.id, &s.thresholds, s.lock.as_ref());
    if !is_triggered(&active_usage, &limits) {
        return Decision::Stay(StayReason::BelowThreshold);
    }
    if waits_for_reset(active, &limits, &targets, s) {
        return Decision::Stay(StayReason::NearReset);
    }

    let tier1: Vec<&&Account> =
        candidates.iter().filter(|a| a.five_hour() < TIER_FIVE_HOUR_LIMIT && a.seven_day() < TIER_SEVEN_DAY_LIMIT).collect();
    if !tier1.is_empty() {
        let scored: Vec<(&&&Account, f64)> = tier1.iter().map(|a| (a, a.usage.map_or(0.0, |u| score(&u, s.now)))).collect();
        let top = scored.iter().map(|(_, sc)| *sc).fold(0.0, f64::max);
        let group: Vec<&&&Account> = scored.iter().filter(|(_, sc)| *sc >= GROUP_RATIO * top).map(|(a, _)| *a).collect();
        let index = pick_index(group.len()).min(group.len() - 1);
        return Decision::SwitchTo(group[index].id.clone(), Reason::Threshold(1));
    }
    // Tier 1 is empty, so every candidate under 99 on 7d has 5h >= 91.
    if let Some(pick) = candidates
        .iter()
        .filter(|a| a.seven_day() < TIER_SEVEN_DAY_LIMIT)
        .min_by(|a, b| a.five_hour().total_cmp(&b.five_hour()))
    {
        return Decision::SwitchTo(pick.id.clone(), Reason::Threshold(2));
    }
    if let Some(pick) = candidates
        .iter()
        .min_by(|a, b| a.seven_day().total_cmp(&b.seven_day()).then(a.five_hour().total_cmp(&b.five_hour())))
    {
        return Decision::SwitchTo(pick.id.clone(), Reason::Threshold(3));
    }
    Decision::Stay(StayReason::NoCandidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_800_000_000.0;
    const DAY: f64 = 86_400.0;
    const MIN: f64 = 60.0;
    const BASE: Thresholds = Thresholds { five_hour: 95.0, seven_day: 99.0 };

    struct A {
        five: f64,
        seven: f64,
        five_resets: Option<f64>,
        seven_resets: Option<f64>,
        availability: Option<Availability>,
        usable: bool,
        active: bool,
        has_usage: bool,
    }

    impl Default for A {
        fn default() -> Self {
            A { five: 0.0, seven: 0.0, five_resets: None, seven_resets: None, availability: Some(Availability::Ready), usable: true, active: false, has_usage: true }
        }
    }

    fn mk(name: &str, a: A) -> Account {
        Account {
            id: name.into(),
            display_name: name.into(),
            usage: a.has_usage.then_some(Usage {
                five_hour: Window { utilization: a.five, resets_at: a.five_resets },
                seven_day: Window { utilization: a.seven, resets_at: a.seven_resets },
            }),
            availability: if a.active { Some(Availability::Active) } else { a.availability },
            is_usable: a.usable,
            is_active: a.active,
            is_fresh: true,
            critical: CriticalAssessment::NONE,
            five_hour_log: Vec::new(),
        }
    }

    fn acct(name: &str, five: f64, seven: f64) -> Account {
        mk(name, A { five, seven, ..A::default() })
    }

    fn active(name: &str, five: f64, seven: f64) -> Account {
        mk(name, A { five, seven, active: true, ..A::default() })
    }

    fn crit(mut a: Account, level: CriticalLevel, waste: f64) -> Account {
        a.critical = CriticalAssessment { level, waste };
        a
    }

    fn snap(accounts: Vec<Account>) -> Snapshot {
        Snapshot { accounts, thresholds: BASE, lock: None, hold: None, priority: None, excluded: HashSet::new(), now: NOW }
    }

    fn with_priority(mut s: Snapshot, id: &str, baseline: Option<f64>) -> Snapshot {
        s.priority = Some(Priority { account_id: id.into(), baseline });
        s
    }

    fn with_hold(mut s: Snapshot, id: &str, interval: f64) -> Snapshot {
        s.hold = Some(Hold { account_id: id.into(), until: NOW + interval });
        s
    }

    fn d(s: &Snapshot) -> Decision {
        decide(s, |_| 0)
    }

    fn to(id: &str, r: Reason) -> Decision {
        Decision::SwitchTo(id.into(), r)
    }

    const T1: Reason = Reason::Threshold(1);
    const STAY_BELOW: Decision = Decision::Stay(StayReason::BelowThreshold);

    fn usage(five: f64, seven: f64) -> Usage {
        Usage { five_hour: Window { utilization: five, resets_at: None }, seven_day: Window { utilization: seven, resets_at: None } }
    }

    // Trigger

    #[test]
    fn stays_below_threshold() {
        assert_eq!(d(&snap(vec![active("a", 94.0, 98.0), acct("b", 0.0, 10.0)])), STAY_BELOW);
    }

    #[test]
    fn triggers_at_exact_thresholds() {
        assert_eq!(d(&snap(vec![active("a", 95.0, 10.0), acct("b", 0.0, 10.0)])), to("b", T1));
        assert_eq!(d(&snap(vec![active("a", 10.0, 99.0), acct("b", 0.0, 10.0)])), to("b", T1));
    }

    #[test]
    fn stays_without_a_usable_active_account() {
        let none = Decision::Stay(StayReason::NoActiveAccount);
        assert_eq!(d(&snap(vec![acct("a", 99.0, 10.0), acct("b", 0.0, 10.0)])), none);
        let no_usage = mk("a", A { active: true, has_usage: false, ..A::default() });
        assert_eq!(d(&snap(vec![no_usage, acct("b", 0.0, 10.0)])), none);
        let mut stale = active("a", 99.0, 10.0);
        stale.is_fresh = false;
        assert_eq!(d(&snap(vec![stale, acct("b", 0.0, 10.0)])), none);
    }

    #[test]
    fn stale_candidate_is_skipped() {
        let mut stale = acct("b", 0.0, 10.0);
        stale.is_fresh = false;
        assert_eq!(d(&snap(vec![active("a", 96.0, 10.0), stale])), Decision::Stay(StayReason::NoCandidate));
    }

    #[test]
    fn warning_and_toggle_zone_edges() {
        assert!(is_in_warning_zone(&usage(92.0, 0.0), &BASE));
        assert!(!is_in_warning_zone(&usage(91.9, 0.0), &BASE));
        assert!(is_in_warning_zone(&usage(0.0, 98.0), &BASE));
        assert!(!is_in_warning_zone(&usage(0.0, 97.9), &BASE));
        assert!(is_in_toggle_zone(&usage(80.0, 0.0), &BASE));
        assert!(!is_in_toggle_zone(&usage(79.9, 0.0), &BASE));
        assert!(is_in_toggle_zone(&usage(0.0, 96.0), &BASE));
        assert!(!is_in_toggle_zone(&usage(0.0, 95.9), &BASE));
    }

    // Candidates

    #[test]
    fn candidate_filters_leave_no_candidate() {
        let mut s = snap(vec![
            active("a", 96.0, 10.0),
            mk("b", A { seven: 10.0, availability: Some(Availability::NotCaptured), ..A::default() }),
            mk("c", A { seven: 10.0, availability: Some(Availability::LoginExpired), ..A::default() }),
            mk("d", A { seven: 10.0, usable: false, ..A::default() }),
            acct("e", 100.0, 10.0),
            acct("f", 0.0, 100.0),
            mk("g", A { has_usage: false, ..A::default() }),
            acct("h", 0.0, 10.0),
            mk("i", A { seven: 10.0, availability: None, ..A::default() }),
        ]);
        s.excluded.insert("h".into());
        assert_eq!(d(&s), Decision::Stay(StayReason::NoCandidate));
    }

    // Tier 1 score and group

    #[test]
    fn score_values() {
        let w = |seven: f64, resets: Option<f64>| Usage { five_hour: Window { utilization: 0.0, resets_at: None }, seven_day: Window { utilization: seven, resets_at: resets } };
        let close = |a: f64, b: f64| assert!((a - b).abs() < 0.001, "{a} != {b}");
        close(score(&w(58.0, Some(NOW + DAY)), NOW), 40.0);
        close(score(&w(8.0, Some(NOW + 6.0 * DAY)), NOW), 15.0);
        assert_eq!(score(&w(10.0, None), NOW), 0.0);
        close(score(&w(10.0, Some(NOW + 9.0 * DAY)), NOW), 88.0 / 7.0);
        close(score(&w(10.0, Some(NOW - DAY)), NOW), 88.0 / 0.25);
        close(score(&w(10.0, Some(NOW + 3600.0)), NOW), 88.0 / 0.25);
        assert_eq!(score(&w(98.5, Some(NOW + DAY)), NOW), 0.0);
    }

    fn week(name: &str, seven: f64, resets_in_days: f64) -> Account {
        mk(name, A { seven, seven_resets: Some(NOW + resets_in_days * DAY), ..A::default() })
    }

    #[test]
    fn late_half_used_clearly_beats_earlier_unused() {
        let mut sizes = Vec::new();
        let s = snap(vec![active("a", 96.0, 10.0), week("unused", 0.0, 3.0), week("late", 50.0, 1.0)]);
        assert_eq!(decide(&s, |n| { sizes.push(n); n - 1 }), to("late", T1));
        assert_eq!(sizes, vec![1]);
    }

    #[test]
    fn random_within_near_top_group() {
        let s = snap(vec![active("a", 96.0, 10.0), week("top", 58.0, 1.0), week("close", 62.0, 1.0), week("far", 8.0, 6.0)]);
        let mut sizes = Vec::new();
        assert_eq!(decide(&s, |n| { sizes.push(n); 0 }), to("top", T1));
        assert_eq!(decide(&s, |n| n - 1), to("close", T1));
        assert_eq!(sizes, vec![2]);
        let mut sizes = Vec::new();
        decide(&snap(vec![active("a", 96.0, 10.0), acct("f1", 0.0, 0.0), acct("f2", 0.0, 0.0)]), |n| { sizes.push(n); 0 });
        assert_eq!(sizes, vec![2], "zero scores make the whole tier the group");
        assert_eq!(decide(&s, |_| 99), to("close", T1), "an out-of-range pick is clamped");
    }

    // Tiers 2 and 3

    #[test]
    fn tier_two_picks_lowest_five_hour() {
        let s = snap(vec![active("a", 96.0, 10.0), acct("b", 93.0, 50.0), acct("c", 92.0, 60.0), acct("d", 10.0, 99.5)]);
        assert_eq!(d(&s), to("c", Reason::Threshold(2)));
    }

    #[test]
    fn tier_three_is_the_last_resort() {
        let s = snap(vec![active("a", 96.0, 10.0), acct("d", 10.0, 99.5), acct("e", 50.0, 99.2), acct("f", 5.0, 99.2)]);
        assert_eq!(d(&s), to("f", Reason::Threshold(3)));
    }

    // Lock

    #[test]
    fn lock_raises_only_the_locked_window() {
        let a = mk("a", A { five: 10.0, seven: 99.5, seven_resets: Some(NOW + DAY), active: true, ..A::default() });
        let mut s = snap(vec![a, acct("b", 0.0, 10.0)]);
        s.lock = Some(Lock { account_id: "a".into(), five_hour: None, seven_day: Some(Some(NOW + DAY)) });
        assert_eq!(d(&s), STAY_BELOW);
        s.accounts[0].usage.as_mut().unwrap().five_hour.utilization = 95.0;
        assert_eq!(d(&s), to("b", T1));
    }

    #[test]
    fn make_lock_covers_only_windows_at_threshold() {
        let u = Usage {
            five_hour: Window { utilization: 93.0, resets_at: Some(NOW + 3600.0) },
            seven_day: Window { utilization: 99.4, resets_at: Some(NOW + DAY) },
        };
        assert_eq!(make_lock("x", &u, &BASE), Some(Lock { account_id: "x".into(), five_hour: None, seven_day: Some(Some(NOW + DAY)) }));
        assert_eq!(make_lock("x", &usage(10.0, 10.0), &BASE), None);
    }

    #[test]
    fn updated_lock_rules() {
        let lock = || Some(Lock { account_id: "a".into(), five_hour: None, seven_day: Some(Some(NOW + DAY)) });
        let used_up = mk("a", A { five: 10.0, seven: 100.0, seven_resets: Some(NOW + DAY), active: true, ..A::default() });
        assert_eq!(updated_lock(lock(), Some(&used_up)), None);
        let reset = mk("a", A { five: 10.0, seven: 2.0, seven_resets: Some(NOW + 7.0 * DAY), active: true, ..A::default() });
        assert_eq!(updated_lock(lock(), Some(&reset)), None);
        let jitter = mk("a", A { five: 10.0, seven: 99.6, seven_resets: Some(NOW + DAY + 0.4), active: true, ..A::default() });
        assert_eq!(updated_lock(lock(), Some(&jitter)), lock());
        let other = active("b", 10.0, 99.6);
        assert_eq!(updated_lock(lock(), Some(&other)), None);
        assert_eq!(updated_lock(lock(), None), None);
        let no_usage = mk("a", A { active: true, has_usage: false, ..A::default() });
        assert_eq!(updated_lock(lock(), Some(&no_usage)), lock());
    }

    // Priority tick

    fn ticked(five: f64, seven: f64, five_resets: Option<f64>) -> Account {
        mk("t", A { five, seven, five_resets, ..A::default() })
    }

    #[test]
    fn priority_rules() {
        let a = || active("a", 10.0, 10.0);
        let pr = Reason::PriorityReset;
        assert_eq!(d(&with_priority(snap(vec![a(), ticked(0.0, 40.0, None)]), "t", Some(NOW - 600.0))), to("t", pr), "after reset, below threshold");
        assert_eq!(d(&with_priority(snap(vec![a(), ticked(2.0, 40.0, Some(NOW + 5.0 * 3600.0))]), "t", Some(NOW - 600.0))), to("t", pr), "later resetsAt");
        assert_eq!(d(&with_priority(snap(vec![a(), ticked(80.0, 40.0, Some(NOW + 600.0))]), "t", Some(NOW + 600.0))), STAY_BELOW, "waits");
        assert_eq!(d(&with_priority(snap(vec![a(), ticked(80.0, 40.0, Some(NOW + 600.4))]), "t", Some(NOW + 600.0))), STAY_BELOW, "drift");
        assert_eq!(d(&with_priority(snap(vec![a(), ticked(0.0, 40.0, None)]), "t", None)), to("t", pr), "nil baseline");
        assert_eq!(d(&with_priority(snap(vec![a(), ticked(0.0, 40.0, None)]), "t", Some(NOW + 3600.0))), STAY_BELOW, "baseline ahead");
        assert_eq!(d(&with_priority(snap(vec![a(), ticked(0.0, 40.0, None)]), "t", Some(NOW - 1.0))), to("t", pr), "baseline passed");
        assert_eq!(d(&with_priority(snap(vec![a(), ticked(0.0, 99.0, None)]), "t", Some(NOW - 600.0))), STAY_BELOW, "7d 99");
        let expired = mk("t", A { seven: 40.0, availability: Some(Availability::LoginExpired), ..A::default() });
        assert_eq!(d(&with_priority(snap(vec![a(), expired]), "t", Some(NOW - 600.0))), STAY_BELOW, "not ready");
    }

    #[test]
    fn priority_baseline_is_the_window_reset_or_five_hours_ahead() {
        let started = Usage { five_hour: Window { utilization: 5.0, resets_at: Some(NOW + 600.0) }, seven_day: Window { utilization: 5.0, resets_at: None } };
        assert_eq!(priority_baseline(&started, NOW), NOW + 600.0);
        assert_eq!(priority_baseline(&usage(0.0, 5.0), NOW), NOW + 5.0 * 3600.0);
    }

    // Critical

    use CriticalLevel::{Critical as C, High as Hi};

    #[test]
    fn critical_rules() {
        let a = || active("a", 10.0, 10.0);
        let b = |level, waste| crit(acct("b", 0.0, 30.0), level, waste);
        assert_eq!(d(&snap(vec![a(), b(Hi, 10.0)])), to("b", Reason::Critical(Hi)), "high pulls from ordinary");
        assert_eq!(d(&with_priority(snap(vec![a(), b(Hi, 10.0)]), "a", None)), to("b", Reason::Critical(Hi)), "high pulls from home");
        assert_eq!(d(&with_priority(snap(vec![a(), b(C, 10.0)]), "a", None)), STAY_BELOW, "critical does not pull from home");
        assert_eq!(d(&snap(vec![a(), b(C, 10.0)])), to("b", Reason::Critical(C)), "critical pulls from ordinary");
        assert_eq!(d(&snap(vec![crit(a(), C, 10.0), b(C, 50.0)])), STAY_BELOW, "critical does not pull from critical");
        assert_eq!(d(&snap(vec![crit(a(), Hi, 10.0), b(Hi, 50.0)])), STAY_BELOW, "high does not pull from high");
        assert_eq!(d(&snap(vec![crit(a(), C, 10.0), b(Hi, 10.0)])), to("b", Reason::Critical(Hi)), "high pulls from critical");
    }

    #[test]
    fn home_and_critical_order() {
        let a = || active("a", 10.0, 10.0);
        let home = || acct("home", 0.0, 10.0);
        let b = |level| crit(acct("b", 0.0, 30.0), level, 10.0);
        let pr = Reason::PriorityReset;
        assert_eq!(d(&with_priority(snap(vec![crit(a(), C, 10.0), home()]), "home", None)), to("home", pr), "home pulls from critical");
        assert_eq!(d(&with_priority(snap(vec![crit(a(), Hi, 10.0), home()]), "home", None)), STAY_BELOW, "home does not pull from high");
        assert_eq!(d(&with_priority(snap(vec![a(), home(), b(Hi)]), "home", None)), to("b", Reason::Critical(Hi)), "high beats home");
        assert_eq!(d(&with_priority(snap(vec![a(), home(), b(C)]), "home", None)), to("home", pr), "home beats critical");
    }

    #[test]
    fn five_hour_at_two_is_free_and_at_three_is_not() {
        let a = || active("a", 10.0, 10.0);
        let two = crit(mk("b", A { five: 2.0, seven: 30.0, five_resets: Some(NOW + 3600.0), ..A::default() }), C, 10.0);
        assert_eq!(d(&snap(vec![a(), two])), to("b", Reason::Critical(C)));
        let three = crit(mk("c", A { five: 3.0, seven: 30.0, five_resets: Some(NOW + 3600.0), ..A::default() }), C, 10.0);
        assert_eq!(d(&snap(vec![a(), three])), STAY_BELOW);
    }

    #[test]
    fn largest_waste_wins_and_target_must_be_a_candidate() {
        let s = snap(vec![active("a", 10.0, 10.0), crit(acct("b", 0.0, 30.0), C, 3.0), crit(acct("c", 0.0, 30.0), C, 9.0)]);
        assert_eq!(d(&s), to("c", Reason::Critical(C)));
        let not_ready = crit(mk("b", A { seven: 30.0, availability: Some(Availability::NotCaptured), ..A::default() }), Hi, 10.0);
        let mut stale = crit(acct("c", 0.0, 30.0), Hi, 10.0);
        stale.is_fresh = false;
        let mut s = snap(vec![active("a", 10.0, 10.0), not_ready, stale, crit(acct("d", 0.0, 30.0), Hi, 10.0)]);
        s.excluded.insert("d".into());
        assert_eq!(d(&s), STAY_BELOW);
    }

    /// Swift's `max(by:)` keeps the first of equal maxima: the earlier account wins a tie.
    #[test]
    fn equal_waste_picks_the_first_target() {
        let s = snap(vec![active("a", 10.0, 10.0), crit(acct("b", 0.0, 30.0), Hi, 10.0), crit(acct("c", 0.0, 30.0), Hi, 10.0)]);
        assert_eq!(d(&s), to("b", Reason::Critical(Hi)));
        let s = snap(vec![active("a", 10.0, 10.0), crit(acct("b", 0.0, 30.0), C, 10.0), crit(acct("c", 0.0, 30.0), C, 10.0)]);
        assert_eq!(d(&s), to("b", Reason::Critical(C)));
    }

    // Hold

    #[test]
    fn hold_rules() {
        let a = mk("a", A { five: 70.0, seven: 66.2, five_resets: Some(NOW + 2.0 * 3600.0), active: true, ..A::default() });
        let c = crit(acct("c", 0.0, 30.0), C, 10.0);
        assert_eq!(d(&with_hold(snap(vec![a.clone(), c.clone()]), "a", 7200.0)), STAY_BELOW, "held account stays");
        assert_eq!(d(&snap(vec![a.clone(), c.clone()])), to("c", Reason::Critical(C)), "without the hold rule 3 moves it");

        let ac = crit(active("a", 40.0, 50.0), C, 10.0);
        let bh = crit(acct("b", 0.0, 30.0), Hi, 10.0);
        assert_eq!(d(&with_hold(snap(vec![ac.clone(), bh.clone()]), "a", 7200.0)), STAY_BELOW);
        assert_eq!(d(&snap(vec![ac, bh])), to("b", Reason::Critical(Hi)));

        let a2 = active("a", 70.0, 60.0);
        assert_eq!(d(&with_hold(snap(vec![a2.clone(), c.clone()]), "a", 0.0)), to("c", Reason::Critical(C)), "expired hold");
        assert_eq!(d(&with_hold(snap(vec![a2.clone(), c.clone()]), "a", -60.0)), to("c", Reason::Critical(C)));
        let ch = crit(acct("c", 0.0, 30.0), Hi, 10.0);
        assert_eq!(d(&with_hold(snap(vec![a2, acct("b", 0.0, 10.0), ch]), "b", 7200.0)), to("c", Reason::Critical(Hi)), "hold on another account");

        assert_eq!(d(&with_hold(snap(vec![active("a", 96.0, 60.0), acct("b", 0.0, 10.0)]), "a", 7200.0)), to("b", T1), "thresholds fire while held");
        let s = with_priority(with_hold(snap(vec![active("a", 40.0, 60.0), acct("home", 0.0, 10.0)]), "a", 7200.0), "home", None);
        assert_eq!(d(&s), to("home", Reason::PriorityReset), "home reset fires while held");
    }

    // Near reset

    /// Claude Code's account at `five`, its 5h window resetting in `left` seconds, with one
    /// log row per `(seconds ago, utilization)` in that window.
    fn near(five: f64, seven: f64, left: f64, log: &[(f64, f64)]) -> Account {
        let resets = NOW + left;
        let mut a = mk("a", A { five, seven, five_resets: Some(resets), active: true, ..A::default() });
        a.five_hour_log = log.iter().map(|&(ago, u)| LogPoint { t: NOW - ago, utilization: u, resets_at: resets }).collect();
        a
    }

    fn home_of(s: Snapshot) -> Snapshot {
        let baseline = s.accounts[0].usage.unwrap().five_hour.resets_at;
        with_priority(s, "a", baseline)
    }

    const NEAR: Decision = Decision::Stay(StayReason::NearReset);

    #[test]
    fn home_waits_for_reset_when_the_pace_fits() {
        let b = || acct("b", 0.0, 10.0);
        assert_eq!(d(&home_of(snap(vec![near(95.0, 10.0, 5.0 * MIN, &[(6.0 * MIN, 91.0), (0.0, 95.0)]), b()]))), NEAR);
        assert_eq!(d(&home_of(snap(vec![near(95.0, 10.0, 5.0 * MIN, &[(6.0 * MIN, 90.0), (0.0, 95.0)]), b()]))), to("b", T1), "pace too fast");
        assert_eq!(d(&home_of(snap(vec![near(95.0, 10.0, 5.0 * MIN + 1.0, &[(7.0 * MIN, 95.0), (0.0, 95.0)]), b()]))), to("b", T1), "more than 5 minutes left");
    }

    #[test]
    fn lookback_is_minutes_left_plus_one() {
        let b = || acct("b", 0.0, 10.0);
        assert_eq!(d(&home_of(snap(vec![near(95.0, 10.0, 2.0 * MIN, &[(10.0 * MIN, 80.0), (3.0 * MIN, 91.0), (0.0, 95.0)]), b()]))), NEAR);
        assert_eq!(d(&home_of(snap(vec![near(95.0, 10.0, 2.0 * MIN, &[(10.0 * MIN, 85.0), (2.5 * MIN, 94.0), (0.0, 95.0)]), b()]))), to("b", T1));
    }

    #[test]
    fn no_usable_log_row_means_no_wait() {
        let b = || acct("b", 0.0, 10.0);
        assert_eq!(d(&home_of(snap(vec![near(95.0, 10.0, 5.0 * MIN, &[]), b()]))), to("b", T1));
        assert_eq!(d(&home_of(snap(vec![near(95.0, 10.0, 5.0 * MIN, &[(3.0 * MIN, 95.0), (0.0, 95.0)]), b()]))), to("b", T1));
        let mut other_window = near(95.0, 10.0, 5.0 * MIN, &[]);
        other_window.five_hour_log = vec![LogPoint { t: NOW - 6.0 * MIN, utilization: 95.0, resets_at: NOW - 3.0 * 3600.0 }];
        assert_eq!(d(&home_of(snap(vec![other_window, b()]))), to("b", T1));
    }

    #[test]
    fn near_reset_with_critical_targets() {
        let a = || near(95.0, 10.0, 5.0 * MIN, &[(6.0 * MIN, 93.0), (0.0, 95.0)]);
        assert_eq!(d(&home_of(snap(vec![a(), crit(acct("c", 0.0, 30.0), C, 10.0)]))), NEAR, "home waits despite a critical target");
        let c = || crit(acct("c", 0.0, 30.0), Hi, 10.0);
        assert_eq!(d(&with_hold(home_of(snap(vec![a(), c()])), "a", 7200.0)), to("c", T1), "held home does not wait for a high target");
        assert_eq!(d(&home_of(snap(vec![a(), c()]))), to("c", Reason::Critical(Hi)));
        let most = crit(a(), C, 20.0);
        assert_eq!(d(&with_hold(snap(vec![most, crit(acct("c", 0.0, 30.0), C, 10.0)]), "a", 7200.0)), NEAR, "most critical waits");
        let less = crit(a(), C, 5.0);
        assert_eq!(d(&with_hold(snap(vec![less, crit(acct("c", 0.0, 30.0), C, 10.0)]), "a", 7200.0)), to("c", T1));
    }

    /// Swift's `max(by:)` keeps the first of two rows logged in the same second.
    #[test]
    fn near_reset_reads_the_first_of_two_rows_in_the_same_second() {
        let b = || acct("b", 0.0, 10.0);
        // First row at 6 minutes ago 91% (fits: 4 <= 4), second at the same time 90% (5 > 4).
        assert_eq!(d(&home_of(snap(vec![near(95.0, 10.0, 5.0 * MIN, &[(6.0 * MIN, 91.0), (6.0 * MIN, 90.0), (0.0, 95.0)]), b()]))), NEAR);
    }

    #[test]
    fn ordinary_account_and_seven_day_still_switch_near_reset() {
        let b = || acct("b", 0.0, 10.0);
        assert_eq!(d(&snap(vec![near(95.0, 10.0, 5.0 * MIN, &[(6.0 * MIN, 95.0), (0.0, 95.0)]), b()])), to("b", T1));
        assert_eq!(d(&home_of(snap(vec![near(95.0, 99.0, 5.0 * MIN, &[(6.0 * MIN, 95.0), (0.0, 95.0)]), b()]))), to("b", T1));
    }
}
