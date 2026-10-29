use std::collections::HashMap;

/// Two `resets_at` this close (seconds) are the same window, as in `usage_ratio`.
pub const RESET_TOLERANCE_S: f64 = 60.0;

/// Minimum seconds between two stored measurements of one key.
const MIN_GAP_S: f64 = 50.0;
/// `A -> C` joins the rate only when it spans at most this many seconds.
const A_WINDOW_S: f64 = 300.0;
/// Rate (%/min) that is a rocket on its own.
const ROCKET_RATE: f64 = 10.0;
/// Rate (%/min) that starts, and must hold for, a sustained streak.
const SUSTAINED_RATE: f64 = 5.0;
/// Seconds a streak must last for `SUSTAINED_RATE` to be a rocket.
const SUSTAINED_S: f64 = 180.0;

/// Level of the rocket, above the ladder's 1-5.
pub const ROCKET_LEVEL: u8 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct BurnRateResult {
    pub level: u8,
    pub animal: &'static str,
    pub projected_time: f64,
}

impl BurnRateResult {
    /// Rocket when the rate is at least 10 %/min, or at least 5 %/min held for
    /// 180 s; otherwise the ladder on the projected seconds to 100%.
    pub fn from_rate(rate_pct_per_min: f64, sustained_s: f64, utilization: f64) -> BurnRateResult {
        let projected = (100.0 - utilization) * 60.0 / rate_pct_per_min;
        if rate_pct_per_min >= ROCKET_RATE
            || (rate_pct_per_min >= SUSTAINED_RATE && sustained_s >= SUSTAINED_S)
        {
            return BurnRateResult { level: ROCKET_LEVEL, animal: "🚀", projected_time: projected };
        }
        Self::from_projected_time(projected)
    }

    pub fn from_projected_time(seconds: f64) -> BurnRateResult {
        let hours = seconds / 3600.0;
        let (level, animal) = if hours > 5.0 {
            (1, "🐌")
        } else if hours > 3.0 {
            (2, "🐢")
        } else if hours > 1.5 {
            (3, "🐇")
        } else if hours > 0.5 {
            (4, "🐎")
        } else {
            (5, "🐆")
        };
        BurnRateResult { level, animal, projected_time: seconds }
    }
}

/// Whether an account's 5h burn level warrants the 60 s fast refresh.
pub fn needs_fast_refresh(
    five_hour_level: Option<u8>,
    is_active: bool,
    has_error: bool,
    auto_refresh_enabled: bool,
) -> bool {
    five_hour_level.is_some_and(|l| l >= 5) && is_active && !has_error && auto_refresh_enabled
}

/// Stateful, in-memory burn-rate tracker. One independent history per
/// (account_id, window) pair, keyed by `"{account_id}_{window}"`.
///
/// `record` implements the rules from `contract/README.md`'s "Burn-rate
/// projection" section, in the exact order the reference implementation
/// evaluates them. It deliberately does NOT reproduce the Swift tracker's
/// unconditional usage-log write on every call - that is caller behaviour
/// (the helper/GUI will call `store::record_usage` separately), kept out of
/// this crate to leave `core`'s projection logic pure and independently
/// testable.
#[derive(Default)]
pub struct BurnRateTracker {
    history: HashMap<String, HistoryEntry>,
}

#[derive(Clone)]
struct Measurement {
    utilization: f64,
    recorded_at: f64,
    resets_at: Option<f64>,
}

#[derive(Default)]
struct HistoryEntry {
    /// At most three, oldest first: A, B, C.
    samples: Vec<Measurement>,
    streak_start: Option<f64>,
    last: Option<BurnRateResult>,
}

impl HistoryEntry {
    fn reset_to(m: Measurement) -> Self {
        Self { samples: vec![m], streak_start: None, last: None }
    }
}

impl BurnRateTracker {
    pub fn record(
        &mut self,
        account_id: &str,
        window: i64,
        utilization: f64,
        resets_at: Option<f64>,
        recorded_at: f64,
    ) -> Option<BurnRateResult> {
        let key = format!("{account_id}_{window}");
        let m = Measurement { utilization, recorded_at, resets_at };
        let e = self.history.entry(key).or_default();

        // First measurement.
        let Some(c) = e.samples.last().cloned() else {
            *e = HistoryEntry::reset_to(m);
            return None;
        };
        // Different reset cycle (more than RESET_TOLERANCE_S apart, or only one
        // side known) or utilization decreased: discard history and streak.
        let same_cycle = match (resets_at, c.resets_at) {
            (Some(a), Some(b)) => (a - b).abs() <= RESET_TOLERANCE_S,
            (a, b) => a == b,
        };
        if !same_cycle || utilization < c.utilization {
            *e = HistoryEntry::reset_to(m);
            return None;
        }
        // Minimum gap: skipped, not stored (also covers non-positive gaps).
        if recorded_at - c.recorded_at < MIN_GAP_S {
            return e.last.clone();
        }
        e.samples.push(m);
        if e.samples.len() > 3 {
            e.samples.remove(0);
        }
        let n = e.samples.len();
        let (b, c) = (&e.samples[n - 2], &e.samples[n - 1]);
        let mut rate = (c.utilization - b.utilization) * 60.0 / (c.recorded_at - b.recorded_at);
        if n == 3 {
            let a = &e.samples[0];
            if c.recorded_at - a.recorded_at <= A_WINDOW_S {
                rate = rate.max((c.utilization - a.utilization) * 60.0 / (c.recorded_at - a.recorded_at));
            }
        }
        e.streak_start = if rate >= SUSTAINED_RATE { e.streak_start.or(Some(b.recorded_at)) } else { None };
        let sustained = c.recorded_at - e.streak_start.unwrap_or(c.recorded_at);
        e.last = (rate > 0.0).then(|| BurnRateResult::from_rate(rate, sustained, c.utilization));
        e.last.clone()
    }
}

#[cfg(test)]
mod tracker_tests {
    use super::*;
    const K: &str = "ACC";
    const W: i64 = 0;

    #[test]
    fn first_measurement_returns_none() {
        let mut t = BurnRateTracker::default();
        assert!(t.record(K, W, 10.0, Some(1000.0), 0.0).is_none());
    }

    #[test]
    fn different_reset_cycle_resets_history() {
        let mut t = BurnRateTracker::default();
        t.record(K, W, 10.0, Some(1000.0), 0.0);
        // resets_at changed -> full reset, None, even though utilization rose
        assert!(t.record(K, W, 20.0, Some(2000.0), 60.0).is_none());
    }

    #[test]
    fn jittered_resets_at_within_tolerance_keeps_cycle() {
        // The API reports a window boundary with sub-second (sometimes whole-second) jitter.
        for jitter in [0.000_024, 0.5, 30.0, -60.0] {
            let mut t = BurnRateTracker::default();
            t.record(K, W, 10.0, Some(1000.0), 0.0);
            let r = t.record(K, W, 20.0, Some(1000.0 + jitter), 100.0);
            assert!(r.is_some(), "jitter {jitter}s is the same cycle");
        }
    }

    #[test]
    fn resets_at_beyond_tolerance_resets_history() {
        let mut t = BurnRateTracker::default();
        t.record(K, W, 10.0, Some(1000.0), 0.0);
        assert!(t.record(K, W, 20.0, Some(1061.0), 100.0).is_none());
        // A known reset time against an unknown one is a different cycle.
        t.record(K, W, 10.0, None, 200.0);
        assert!(t.record(K, W, 20.0, Some(1061.0), 300.0).is_none());
    }

    #[test]
    fn utilization_increase_projects() {
        let mut t = BurnRateTracker::default();
        t.record(K, W, 10.0, Some(1000.0), 0.0);
        // +10% over 100s => rate 0.1%/s, remaining 80 => 800s projected.
        // 800s = 0.222h, which fails every ">" threshold down to ">0.5"
        // (1800s) -> level 5, per contract/cases/burn-rate-levels.json's
        // "exactly 30m is level 5, not 4" (1800s -> level 5; 800 < 1800).
        let r = t.record(K, W, 20.0, Some(1000.0), 100.0).unwrap();
        assert_eq!(r.projected_time, 800.0);
        assert_eq!(r.level, 5);
    }

    #[test]
    fn utilization_decrease_resets_returns_none() {
        let mut t = BurnRateTracker::default();
        t.record(K, W, 20.0, Some(1000.0), 0.0);
        assert!(t.record(K, W, 10.0, Some(1000.0), 60.0).is_none());
    }

    #[test]
    fn increase_with_nonpositive_dt_returns_none_without_touching_history() {
        let mut t = BurnRateTracker::default();
        t.record(K, W, 10.0, Some(1000.0), 100.0);
        assert!(t.record(K, W, 20.0, Some(1000.0), 100.0).is_none()); // dt=0
    }

    #[test]
    fn skipped_measurement_keeps_history() {
        let mut t = BurnRateTracker::default();
        t.record(K, W, 10.0, Some(1000.0), 0.0);
        let first = t.record(K, W, 11.0, Some(1000.0), 60.0).unwrap();
        // 49 s after C: not stored, last result returned.
        assert_eq!(t.record(K, W, 30.0, Some(1000.0), 109.0), Some(first));
        // The next step computes from the old C (11 at 60), not from the skipped one.
        let r = t.record(K, W, 12.0, Some(1000.0), 120.0).unwrap();
        assert_eq!(r.projected_time, 5280.0);
    }

    #[test]
    fn nil_return_records_nil_as_last_result() {
        let mut t = BurnRateTracker::default();
        assert!(t.record(K, W, 10.0, Some(1000.0), 0.0).is_none());
        // Within 50 s of the first measurement: returns the remembered nil.
        assert!(t.record(K, W, 20.0, Some(1000.0), 30.0).is_none());
        // A rate-0 result also becomes the remembered nil.
        t.record(K, W, 20.0, Some(1000.0), 60.0).unwrap();
        // Flat, and A is beyond 300 s: rate 0.
        assert!(t.record(K, W, 20.0, Some(1000.0), 400.0).is_none());
        assert!(t.record(K, W, 25.0, Some(1000.0), 410.0).is_none());
    }

    #[test]
    fn streak_resets_with_history() {
        let mut t = BurnRateTracker::default();
        t.record(K, W, 0.0, Some(1000.0), 0.0);
        t.record(K, W, 5.0, Some(1000.0), 60.0);
        t.record(K, W, 10.0, Some(1000.0), 120.0);
        // Rate 5 held since t=0: rocket at 180 s.
        assert_eq!(t.record(K, W, 15.0, Some(1000.0), 180.0).unwrap().level, 6);
        // Cycle change clears samples and streak.
        assert!(t.record(K, W, 20.0, Some(2000.0), 240.0).is_none());
        t.record(K, W, 25.0, Some(2000.0), 300.0);
        // Streak restarts at 240, so only 120 s sustained at 360.
        assert_eq!(t.record(K, W, 30.0, Some(2000.0), 360.0).unwrap().level, 5);
    }

    #[test]
    fn needs_fast_refresh_requires_every_condition() {
        assert!(needs_fast_refresh(Some(5), true, false, true));
        assert!(needs_fast_refresh(Some(ROCKET_LEVEL), true, false, true));
        assert!(!needs_fast_refresh(Some(4), true, false, true));
        assert!(!needs_fast_refresh(None, true, false, true));
        assert!(!needs_fast_refresh(Some(5), false, false, true));
        assert!(!needs_fast_refresh(Some(5), true, true, true));
        assert!(!needs_fast_refresh(Some(5), true, false, false));
    }
}
