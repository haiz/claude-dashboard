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
}
