//! Chart arithmetic, ported verbatim from `apps/linux/lib/chart.js`.
//! All times are milliseconds. No locale or timezone lookups: `format_tick`
//! renders the wall clock of the ms it is given; a caller wanting local time
//! passes ms already shifted by the local UTC offset.

use std::collections::BTreeMap;

pub const Y_MIN: f64 = 0.0;
pub const Y_MAX: f64 = 100.0;
pub const Y_TICKS: [f64; 5] = [0.0, 25.0, 50.0, 75.0, 100.0];

const DAY_MS: f64 = 86_400_000.0;
const MIN_SPAN_MS: f64 = 60_000.0;
const MAX_SPAN_MS: f64 = 90.0 * DAY_MS;

const TIME_STEPS_MS: [f64; 11] = [
    60_000.0,
    5.0 * 60_000.0,
    15.0 * 60_000.0,
    30.0 * 60_000.0,
    3_600_000.0,
    3.0 * 3_600_000.0,
    6.0 * 3_600_000.0,
    12.0 * 3_600_000.0,
    86_400_000.0,
    2.0 * 86_400_000.0,
    7.0 * 86_400_000.0,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub from_ms: f64,
    pub to_ms: f64,
}

/// Plot geometry. Mirrors the object `makeScale` returns; `x`/`y` are methods.
#[derive(Debug, Clone, Copy)]
pub struct Scale {
    pub left: f64,
    pub top: f64,
    pub plot_width: f64,
    pub plot_height: f64,
    pub from_ms: f64,
    pub to_ms: f64,
    pub right: f64,
    pub bottom: f64,
    span: f64,
}

/// `padding` is applied uniformly to all four sides.
pub fn make_scale(range: Range, width: f64, height: f64, padding: f64) -> Scale {
    let plot_width = (width - padding * 2.0).max(1.0);
    let plot_height = (height - padding * 2.0).max(1.0);
    let span = (range.to_ms - range.from_ms).max(1.0);
    Scale {
        left: padding,
        top: padding,
        plot_width,
        plot_height,
        from_ms: range.from_ms,
        to_ms: range.to_ms,
        right: padding + plot_width,
        bottom: padding + plot_height,
        span,
    }
}

impl Scale {
    pub fn x(&self, ms: f64) -> f64 {
        self.left + ((ms - self.from_ms) / self.span) * self.plot_width
    }

    pub fn y(&self, value: f64) -> f64 {
        let clamped = value.clamp(Y_MIN, Y_MAX);
        self.top + (1.0 - (clamped - Y_MIN) / (Y_MAX - Y_MIN)) * self.plot_height
    }

    pub fn ms_at(&self, px: f64) -> f64 {
        self.from_ms + ((px - self.left) / self.plot_width) * self.span
    }

    pub fn value_at(&self, py: f64) -> f64 {
        Y_MIN + (1.0 - (py - self.top) / self.plot_height) * (Y_MAX - Y_MIN)
    }
}

pub fn time_ticks(from_ms: f64, to_ms: f64, max_ticks: usize) -> Vec<f64> {
    let span = (to_ms - from_ms).max(1.0);
    let step = TIME_STEPS_MS
        .iter()
        .copied()
        .find(|s| span / s <= max_ticks as f64)
        .unwrap_or(TIME_STEPS_MS[TIME_STEPS_MS.len() - 1]);
    let mut t = (from_ms / step).ceil() * step;
    let mut ticks = Vec::new();
    while t <= to_ms {
        ticks.push(t);
        t += step;
    }
    ticks
}

/// "HH:MM" below a day of span, else "Ddd HH:MM" (wall clock of `ms`).
pub fn format_tick(ms: f64, span_ms: f64) -> String {
    let total_min = (ms / 60_000.0).floor() as i64;
    let minute = total_min.rem_euclid(60);
    let hour = (total_min.div_euclid(60)).rem_euclid(24);
    let days = total_min.div_euclid(60 * 24);
    if span_ms < DAY_MS {
        return format!("{hour:02}:{minute:02}");
    }
    // 1970-01-01 was a Thursday (index 4 of Sun-first).
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let day = DAYS[(days + 4).rem_euclid(7) as usize];
    format!("{day} {hour:02}:{minute:02}")
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub ms: f64,
    pub value: f64,
}

pub fn nearest_entry(entries: &[Entry], ms: f64) -> Option<usize> {
    if entries.is_empty() {
        return None;
    }
    let mut best = 0;
    let mut best_distance = (entries[0].ms - ms).abs();
    for (i, e) in entries.iter().enumerate() {
        let d = (e.ms - ms).abs();
        if d < best_distance {
            best = i;
            best_distance = d;
        }
    }
    Some(best)
}

/// chart.js splits on a reset, not a time gap: a sample at exactly 0 following
/// a non-zero one closes the current segment (and belongs to it).
pub fn segments(entries: &[Entry]) -> Vec<Vec<Entry>> {
    let mut out = Vec::new();
    let mut current: Vec<Entry> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let reset = i > 0 && entries[i - 1].value > 0.0 && entry.value == 0.0;
        current.push(*entry);
        if reset {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn zoom_range(range: Range, factor: f64, anchor_ms: f64) -> Range {
    let span = range.to_ms - range.from_ms;
    let next_span = (span * factor).clamp(MIN_SPAN_MS, MAX_SPAN_MS);
    let anchor_fraction = if span == 0.0 {
        0.5
    } else {
        (anchor_ms - range.from_ms) / span
    };
    let next_from = anchor_ms - anchor_fraction * next_span;
    Range {
        from_ms: next_from,
        to_ms: next_from + next_span,
    }
}

pub fn pan_range(range: Range, delta_ms: f64) -> Range {
    Range {
        from_ms: range.from_ms + delta_ms,
        to_ms: range.to_ms + delta_ms,
    }
}

pub fn full_range(entries: &[Entry], now_ms: f64) -> Range {
    if entries.is_empty() {
        return Range {
            from_ms: now_ms - DAY_MS,
            to_ms: now_ms,
        };
    }
    let min = entries.iter().map(|e| e.ms).fold(f64::INFINITY, f64::min);
    let max = entries.iter().map(|e| e.ms).fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(MIN_SPAN_MS);
    let margin = span * 0.02;
    Range {
        from_ms: min - margin,
        to_ms: max + margin,
    }
}

/// `a`/`b` are (ms, value) points. Returns (delta_value, delta_ms). chart.js
/// also returns perHour; see `measure_per_hour`.
pub fn measure(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (b.1 - a.1, b.0 - a.0)
}

/// chart.js `perHour`: None for a zero or negative time gap.
pub fn measure_per_hour(a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let (dv, dms) = measure(a, b);
    if dms > 0.0 {
        Some(dv / (dms / 3_600_000.0))
    } else {
        None
    }
}

/// Summed value across accounts at each distinct timestamp, carrying each
/// account's last known value forward. Entries per account are assumed sorted.
pub fn total_series(by_account: &BTreeMap<String, Vec<Entry>>) -> Vec<Entry> {
    let mut stamps: Vec<f64> = by_account
        .values()
        .flat_map(|v| v.iter().map(|e| e.ms))
        .collect();
    stamps.sort_by(|a, b| a.total_cmp(b));
    stamps.dedup();

    let mut cursors: BTreeMap<&String, usize> = by_account.keys().map(|k| (k, 0)).collect();
    let mut last: BTreeMap<&String, Option<f64>> = by_account.keys().map(|k| (k, None)).collect();

    stamps
        .into_iter()
        .map(|ms| {
            let mut total = 0.0;
            for (key, entries) in by_account {
                let mut i = cursors[key];
                while i < entries.len() && entries[i].ms <= ms {
                    last.insert(key, Some(entries[i].value));
                    i += 1;
                }
                cursors.insert(key, i);
                if let Some(v) = last[key] {
                    total += v;
                }
            }
            Entry { ms, value: total }
        })
        .collect()
}

/// Average burn rate (%/h) over `[from_ms, to_ms]`: sum of positive deltas
/// between consecutive visible points (drops are resets, skipped) divided by
/// the range in hours. Port of macOS `computeAverageRate`; needs >= 2 points.
pub fn average_rate(entries: &[Entry], from_ms: f64, to_ms: f64) -> Option<f64> {
    let mut visible: Vec<Entry> = entries.iter().copied().filter(|e| e.ms >= from_ms && e.ms <= to_ms).collect();
    if visible.len() < 2 {
        return None;
    }
    visible.sort_by(|a, b| a.ms.total_cmp(&b.ms));
    let sum: f64 = visible.windows(2).map(|w| (w[1].value - w[0].value).max(0.0)).sum();
    let hours = (to_ms - from_ms) / 3_600_000.0;
    if hours > 0.0 { Some(sum / hours) } else { None }
}

/// Plan-weighted mean burn rate across accounts, port of macOS
/// `OverviewChartView.weightedAverageRate`. Each account is `(entries, plan
/// weight)` (pro 1, max5x 5, max20x 20, max200 10); entries are ascending.
/// Only accounts with at least one point in range contribute (a lone point
/// contributes 0 delta but still counts its weight). `None` when the range is
/// <= 0.01 h or no account has a point in range.
pub fn weighted_average_rate(accounts: &[(&[Entry], f64)], from_ms: f64, to_ms: f64) -> Option<f64> {
    let hours = (to_ms - from_ms) / 3_600_000.0;
    if hours <= 0.01 {
        return None;
    }
    let mut weighted = 0.0;
    let mut total_weight = 0.0;
    for (entries, weight) in accounts {
        let mut last: Option<f64> = None;
        let mut delta = 0.0;
        for e in entries.iter().filter(|e| e.ms >= from_ms && e.ms <= to_ms) {
            if let Some(p) = last {
                delta += (e.value - p).max(0.0);
            }
            last = Some(e.value);
        }
        if last.is_some() {
            weighted += delta * weight;
            total_weight += weight;
        }
    }
    if total_weight > 0.0 { Some(weighted / total_weight / hours) } else { None }
}

/// Local-noon instants (ms) inside `[from_ms, to_ms]`, one per local day.
/// `local_offset_ms` is the local UTC offset (UTC+7 = 25_200_000).
pub fn noon_markers(from_ms: f64, to_ms: f64, local_offset_ms: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut day = ((from_ms + local_offset_ms) / DAY_MS).floor() * DAY_MS - local_offset_ms;
    while day <= to_ms {
        let noon = day + 12.0 * 3_600_000.0;
        if noon >= from_ms && noon <= to_ms {
            out.push(noon);
        }
        day += DAY_MS;
    }
    out
}

/// The entries flagged limited (macOS draws a red point for each `isLimited` log).
pub fn limit_points(entries: &[(Entry, bool)]) -> Vec<Entry> {
    entries.iter().filter(|(_, l)| *l).map(|(e, _)| *e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1_700_000_000_000.0;
    fn e(ms: f64, value: f64) -> Entry {
        Entry { ms, value }
    }

    #[test]
    fn scale_and_fullrange_handle_empty() {
        let s = make_scale(Range { from_ms: 5.0, to_ms: 5.0 }, 0.0, 0.0, 10.0);
        assert!(s.x(5.0).is_finite() && s.y(50.0).is_finite());
        assert!(s.ms_at(10.0).is_finite());
        assert_eq!(s.plot_width, 1.0);
        let r = full_range(&[], 1_000_000_000.0);
        assert_eq!(r, Range { from_ms: 1_000_000_000.0 - 86_400_000.0, to_ms: 1_000_000_000.0 });
        let one = full_range(&[e(1_000_000.0, 1.0)], 0.0);
        assert_eq!(one, Range { from_ms: 1_000_000.0 - 1200.0, to_ms: 1_000_000.0 + 1200.0 });
    }

    #[test]
    fn make_scale_maps_known_point() {
        let s = make_scale(Range { from_ms: 1000.0, to_ms: 2000.0 }, 220.0, 120.0, 10.0);
        assert_eq!((s.plot_width, s.plot_height, s.right, s.bottom), (200.0, 100.0, 210.0, 110.0));
        assert_eq!(s.x(1500.0), 110.0);
        assert_eq!(s.x(1000.0), 10.0);
        assert_eq!(s.y(100.0), 10.0);
        assert_eq!(s.y(0.0), 110.0);
        assert_eq!(s.y(25.0), 85.0);
        assert_eq!(s.y(150.0), 10.0); // clamped
        assert_eq!(s.ms_at(110.0), 1500.0);
        assert_eq!(s.value_at(85.0), 25.0);
    }

    #[test]
    fn zoom_range_clamps_min_and_max() {
        let r = Range { from_ms: 0.0, to_ms: 1_000_000.0 };
        let z = zoom_range(r, 0.0001, 500_000.0);
        assert_eq!(z.to_ms - z.from_ms, 60_000.0);
        assert_eq!(z.from_ms, 470_000.0); // anchor keeps its 0.5 fraction
        let z = zoom_range(r, 1e9, 500_000.0);
        assert_eq!(z.to_ms - z.from_ms, 90.0 * 86_400_000.0);
        let z = zoom_range(r, 0.5, 250_000.0);
        assert_eq!(z, Range { from_ms: 125_000.0, to_ms: 625_000.0 });
        let z = zoom_range(Range { from_ms: 10.0, to_ms: 10.0 }, 1.0, 10.0);
        assert!(z.from_ms < z.to_ms);
    }

    #[test]
    fn pan_shifts_both_ends() {
        let p = pan_range(Range { from_ms: 1.0, to_ms: 5.0 }, 3.0);
        assert_eq!(p, Range { from_ms: 4.0, to_ms: 8.0 });
    }

    #[test]
    fn nearest_entry_picks_closest() {
        let es = [e(T0, 10.0), e(T0 + 100_000.0, 20.0), e(T0 + 500_000.0, 30.0)];
        assert_eq!(nearest_entry(&es, T0 + 100_000.0), Some(1));
        assert_eq!(nearest_entry(&es, T0 + 40_000.0), Some(0));
        assert_eq!(nearest_entry(&es, T0 + 400_000.0), Some(2));
        assert_eq!(nearest_entry(&es, T0 + 50_000.0), Some(0)); // midpoint: first wins
        assert_eq!(nearest_entry(&[], 1.0), None);
    }

    #[test]
    fn segments_split_on_gap() {
        // chart.js splits on a reset to exactly 0 after a non-zero sample.
        let es = [e(0.0, 5.0), e(1.0, 9.0), e(2.0, 0.0), e(3.0, 4.0), e(4.0, 6.0)];
        let segs = segments(&es);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0], vec![es[0], es[1], es[2]]);
        assert_eq!(segs[1], vec![es[3], es[4]]);
        assert_eq!(segments(&[e(0.0, 1.0), e(1000.0, 2.0)]).len(), 1);
        assert_eq!(segments(&[]).len(), 0);
        assert_eq!(segments(&[e(0.0, 0.0), e(1.0, 5.0)]).len(), 1);
    }

    #[test]
    fn total_series_merges_with_carry_forward() {
        let mut m = BTreeMap::new();
        m.insert("a".to_string(), vec![e(1.0, 10.0), e(3.0, 20.0)]);
        m.insert("b".to_string(), vec![e(2.0, 5.0), e(3.0, 7.0)]);
        assert_eq!(total_series(&m), vec![e(1.0, 10.0), e(2.0, 15.0), e(3.0, 27.0)]);
        assert!(total_series(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn time_ticks_literal() {
        assert_eq!(
            time_ticks(0.0, 3_600_000.0, 6),
            vec![0.0, 900_000.0, 1_800_000.0, 2_700_000.0, 3_600_000.0]
        );
        let wide = time_ticks(0.0, 365.0 * 86_400_000.0, 6);
        assert_eq!(wide[1] - wide[0], 7.0 * 86_400_000.0);
        let t = time_ticks(T0, T0 + 6.0 * 3_600_000.0, 6);
        assert!(t.iter().all(|x| *x >= T0 && *x <= T0 + 6.0 * 3_600_000.0));
        assert_eq!(t[1] - t[0], 3_600_000.0);
    }

    #[test]
    fn format_tick_literal() {
        // 2026-01-05 14:30 (a Monday) as wall-clock ms.
        let ms = 1_767_623_400_000.0;
        assert_eq!(format_tick(ms, 3_600_000.0), "14:30");
        assert_eq!(format_tick(ms, 2.0 * 86_400_000.0), "Mon 14:30");
        assert_eq!(format_tick(0.0, 86_400_000.0), "Thu 00:00");
    }

    #[test]
    fn measure_literal() {
        assert_eq!(measure((0.0, 10.0), (3_600_000.0, 25.0)), (15.0, 3_600_000.0));
        assert_eq!(measure_per_hour((0.0, 10.0), (3_600_000.0, 25.0)), Some(15.0));
        assert_eq!(measure_per_hour((5.0, 1.0), (5.0, 2.0)), None);
    }

    fn ent(h: f64, v: f64) -> Entry {
        Entry { ms: h * 3_600_000.0, value: v }
    }

    #[test]
    fn average_rate_skips_reset_drop() {
        let pts = [ent(0.0, 10.0), ent(1.0, 30.0), ent(2.0, 0.0), ent(3.0, 20.0)];
        let r = average_rate(&pts, 0.0, 3.0 * 3_600_000.0).unwrap();
        assert!((r - 40.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn average_rate_needs_two_points() {
        assert_eq!(average_rate(&[ent(1.0, 5.0)], 0.0, 3_600_000.0), None);
        assert_eq!(average_rate(&[], 0.0, 3_600_000.0), None);
    }

    #[test]
    fn noon_markers_one_per_day() {
        let off = 7.0 * 3_600_000.0;
        // Local midnight of day 10 (UTC+7) = 10 days - 7 h in UTC ms.
        let start = 10.0 * DAY_MS - off;
        let m = noon_markers(start, start + 3.0 * DAY_MS, off);
        assert_eq!(m, vec![start + 12.0 * 3_600_000.0, start + 36.0 * 3_600_000.0, start + 60.0 * 3_600_000.0]);
    }

    #[test]
    fn limit_points_filters_flagged() {
        let v = [(ent(0.0, 1.0), false), (ent(1.0, 100.0), true)];
        assert_eq!(limit_points(&v), vec![ent(1.0, 100.0)]);
    }

    // Hand-run of the macOS formula over a 10 h range.
    // A (weight 1): 10,30,20,50 -> deltas 20+0+30 = 50
    // B (weight 5): 0,10 -> delta 10
    // C (weight 20): one point only -> delta 0, but weight counts
    // weighted = 50*1 + 10*5 + 0*20 = 100; weight = 26; 100/26/10.
    #[test]
    fn weighted_average_rate_literal() {
        let a = [ent(1.0, 10.0), ent(2.0, 30.0), ent(3.0, 20.0), ent(4.0, 50.0)];
        let b = [ent(1.0, 0.0), ent(2.0, 10.0)];
        let c = [ent(5.0, 70.0)];
        let r = weighted_average_rate(&[(&a, 1.0), (&b, 5.0), (&c, 20.0)], 0.0, 10.0 * 3_600_000.0).unwrap();
        assert!((r - 100.0 / 26.0 / 10.0).abs() < 1e-9);
        assert_eq!(weighted_average_rate(&[(&a, 1.0)], 0.0, 30_000.0), None);
        assert_eq!(weighted_average_rate(&[(&a, 1.0)], 50.0 * 3_600_000.0, 60.0 * 3_600_000.0), None);
    }
}
