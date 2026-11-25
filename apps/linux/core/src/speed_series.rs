//! Speed series for the Overview "Speed" chart: how fast an account's usage rose, per
//! local-clock bucket, in Max-equivalent percent per hour. Rules in `contract/README.md`,
//! "Speed series"; cases in `contract/cases/speed-series.json`
//! (`tests/contract_speed_series.rs`).
//!
//! ```
//! use claude_dashboard_core::chart::Entry;
//! use claude_dashboard_core::speed_series::{account_speed, max_factor};
//! // One 1% rise inside the 00:00-00:06 bucket reads 10 %/h for a Max account.
//! let e = [Entry { ms: 0.0, value: 5.0 }, Entry { ms: 120_000.0, value: 5.0 },
//!          Entry { ms: 240_000.0, value: 6.0 }, Entry { ms: 720_000.0, value: 6.0 }];
//! let p = account_speed(&e, 360_000.0, 0.0, 0.0, 720_000.0, max_factor("Max"));
//! assert_eq!(p[0].end_ms, 360_000.0);
//! assert!((p[0].value - 10.0).abs() < 1e-9);
//! ```

use crate::chart::Entry;
use std::collections::BTreeMap;

const HOUR_MS: f64 = 3_600_000.0;
/// An account whose newest entry is at most this old is "live" for [`total_speed`].
const LIVE_MS: f64 = 900_000.0;

/// Plan weight by label ("Pro", "Max 5x", "Max 20x", "Max", "Team", "Enterprise",
/// "Free"); an unknown label weighs 1. Also the Avg overlay's weight.
pub fn plan_weight(plan: &str) -> f64 {
    match plan {
        "Max 5x" => 5.0,
        "Max 20x" => 20.0,
        "Max" => 10.0,
        _ => 1.0,
    }
}

/// Scale that turns an account's speed into Max-equivalent %/h: `plan_weight / 10`.
pub fn max_factor(plan: &str) -> f64 {
    plan_weight(plan) / 10.0
}

/// One bucket's speed, stamped at the bucket end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedPoint {
    pub end_ms: f64,
    pub value: f64,
}

/// Per-bucket speed of one account, scaled by `factor`.
///
/// `entries` are ascending and already through the reset-transition pass. Buckets are
/// `step_ms` wide, aligned to the local clock (`utc_offset_ms`), and are the ones
/// overlapping `[from_ms, to_ms]`. A bucket gets a point only when the entries cover it
/// whole (first entry at or before its start, last at or after its end).
pub fn account_speed(
    entries: &[Entry],
    step_ms: f64,
    utc_offset_ms: f64,
    from_ms: f64,
    to_ms: f64,
    factor: f64,
) -> Vec<SpeedPoint> {
    let (Some(first), Some(last)) = (entries.first(), entries.last()) else {
        return Vec::new();
    };
    let index = |t: f64| ((t + utc_offset_ms) / step_ms).floor() as i64;
    let start = |k: i64| k as f64 * step_ms - utc_offset_ms;

    let k_first = index(from_ms);
    let k_last = ((to_ms + utc_offset_ms) / step_ms).ceil() as i64 - 1;
    if k_last < k_first {
        return Vec::new();
    }
    // Index 0 is bucket k_first; only covered buckets are kept at the end.
    let mut sums = vec![0.0_f64; (k_last - k_first + 1) as usize];
    let slot = |k: i64| (k >= k_first && k <= k_last).then(|| (k - k_first) as usize);

    for pair in entries.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let rise = b.value - a.value;
        if rise <= 0.0 {
            continue;
        }
        if b.ms == a.ms {
            if let Some(i) = slot(index(b.ms)) {
                sums[i] += rise;
            }
            continue;
        }
        let span = b.ms - a.ms;
        for k in index(a.ms).max(k_first)..=index(b.ms).min(k_last) {
            let overlap = (start(k + 1).min(b.ms) - start(k).max(a.ms)).max(0.0);
            sums[(k - k_first) as usize] += rise * overlap / span;
        }
    }

    (k_first..=k_last)
        .filter(|&k| first.ms <= start(k) && last.ms >= start(k + 1))
        .map(|k| SpeedPoint {
            end_ms: start(k + 1),
            value: sums[(k - k_first) as usize] / (step_ms / HOUR_MS) * factor,
        })
        .collect()
}

/// One account's already scaled points and the time of its newest entry.
pub struct AccountSpeed<'a> {
    pub points: &'a [SpeedPoint],
    pub last_entry_ms: f64,
}

/// Total speed: per bucket end, the sum over the accounts that have a point there.
///
/// An account is live when `now_ms - last_entry_ms <= 15 min`. While any live account has
/// points, the Total stops at the earliest last point end among them, so it never draws
/// a bucket a live account has not reached. Stale accounts do not hold it back.
pub fn total_speed(accounts: &[AccountSpeed], now_ms: f64) -> Vec<SpeedPoint> {
    let mut sums: BTreeMap<i64, SpeedPoint> = BTreeMap::new();
    for a in accounts {
        for p in a.points {
            sums.entry(p.end_ms as i64)
                .or_insert(SpeedPoint { end_ms: p.end_ms, value: 0.0 })
                .value += p.value;
        }
    }
    let cutoff = accounts
        .iter()
        .filter(|a| now_ms - a.last_entry_ms <= LIVE_MS)
        .filter_map(|a| a.points.last().map(|p| p.end_ms))
        .reduce(f64::min);
    sums.into_values()
        .filter(|p| cutoff.map_or(true, |c| p.end_ms <= c))
        .collect()
}
