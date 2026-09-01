use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct BurnRateResult {
    pub level: u8,
    pub animal: &'static str,
    pub projected_time: f64,
}

impl BurnRateResult {
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

/// Stateful, in-memory burn-rate tracker. One independent history per
/// (account_id, window) pair, keyed by `"{account_id}_{window}"`.
///
/// `record` implements the five rules from `contract/README.md`'s
/// "Burn-rate projection" section, in the exact order the reference
/// implementation evaluates them. It deliberately does NOT reproduce the
/// Swift tracker's unconditional usage-log write on every call — that is
/// caller behaviour (the helper/GUI will call `store::record_usage`
/// separately in a later plan), kept out of this crate to leave `core`'s
/// projection logic pure and independently testable.
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
    prev: Option<Measurement>,
    current: Option<Measurement>,
    last_rate: Option<f64>,
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

        // Rule 1: first measurement — no entry, or entry's `current` is None.
        let Some(current) = e.current.clone() else {
            *e = HistoryEntry { prev: None, current: Some(m), last_rate: None };
            return None;
        };
        // Rule 2: different reset cycle — exact Option<f64> equality.
        if resets_at != current.resets_at {
            *e = HistoryEntry { prev: None, current: Some(m), last_rate: None };
            return None;
        }
        // Rule 3: utilization decreased — full reset, same as rule 2.
        if utilization < current.utilization {
            *e = HistoryEntry { prev: None, current: Some(m), last_rate: None };
            return None;
        }
        // Rule 4: utilization increased.
        if utilization > current.utilization {
            let dt = recorded_at - current.recorded_at;
            if dt <= 0.0 {
                // Discard the new measurement without touching history.
                return None;
            }
            let rate = (utilization - current.utilization) / dt; // %/s, > 0
            let projected = (100.0 - utilization) / rate;
            e.prev = Some(current);
            e.current = Some(m);
            e.last_rate = Some(rate);
            return Some(BurnRateResult::from_projected_time(projected));
        }
        // Rule 5: utilization unchanged — exact f64 equality (neither rule 3
        // nor rule 4 fired).
        let gap = recorded_at - current.recorded_at;
        if gap >= 300.0 {
            // Stale: advance current, drop the carried rate, keep prev.
            e.current = Some(m);
            e.last_rate = None;
            return None;
        }
        // Fresh (gap < 300, including negative gaps from out-of-order polls).
        match e.last_rate {
            None => {
                e.current = Some(m);
                None
            }
            Some(rate) => {
                let remaining = 100.0 - utilization;
                if remaining <= 0.0 {
                    // Level 5, without updating history — current stays the
                    // older measurement.
                    Some(BurnRateResult::from_projected_time(0.0))
                } else {
                    let projected = remaining / rate;
                    e.current = Some(m); // prev + last_rate carried unchanged
                    Some(BurnRateResult::from_projected_time(projected))
                }
            }
        }
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
    fn unchanged_stale_gap_ge_300_drops_rate() {
        let mut t = BurnRateTracker::default();
        t.record(K, W, 10.0, Some(1000.0), 0.0);
        t.record(K, W, 20.0, Some(1000.0), 100.0); // establishes lastRate
        // unchanged, gap 300 exactly => stale => None, rate dropped
        assert!(t.record(K, W, 20.0, Some(1000.0), 400.0).is_none());
    }
}
