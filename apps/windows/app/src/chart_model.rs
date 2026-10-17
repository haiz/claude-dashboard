//! Pure chart model for the Slint UI: Path strings, axis ticks and hover
//! hit-testing. All geometry comes from `claude_dashboard_core::chart`.

use claude_dashboard_core::chart::{
    format_tick, segments, time_ticks, Entry, Range, Scale, Y_TICKS,
};
use claude_dashboard_core::store::SeriesPoint;

/// (x ticks as (pixel, label), y gridlines as (value, pixel)).
pub type AxisTicks = (Vec<(f64, String)>, Vec<(f64, f64)>);

const HOUR_MS: f64 = 3_600_000.0;
const DAY_MS: f64 = 86_400_000.0;

/// A range preset's duration (ms), as macOS `TimeRangePreset.seconds`. The
/// usage window (5h / 7d / Fable) is chosen separately; unknown presets fall
/// back to 24h.
pub fn preset_span_ms(preset: &str) -> f64 {
    match preset {
        "5h" => 5.0 * HOUR_MS,
        "3d" => 3.0 * DAY_MS,
        "7d" => 7.0 * DAY_MS,
        "30d" => 30.0 * DAY_MS,
        _ => DAY_MS,
    }
}

/// Linear interpolation of the series at `ms` (macOS `interpolateValue`):
/// exact hit, else between the neighbours, else the nearest edge value.
/// `entries` ascending by time.
pub fn interpolate(entries: &[Entry], ms: f64) -> Option<f64> {
    if entries.is_empty() {
        return None;
    }
    let before = entries.iter().rposition(|e| e.ms <= ms);
    let after = entries.iter().position(|e| e.ms >= ms);
    match (before, after) {
        (Some(b), Some(a)) if entries[b].ms != entries[a].ms => {
            let (eb, ea) = (entries[b], entries[a]);
            let f = (ms - eb.ms) / (ea.ms - eb.ms);
            Some(eb.value + (ea.value - eb.value) * f)
        }
        (Some(b), _) => Some(entries[b].value),
        (None, Some(a)) => Some(entries[a].value),
        _ => None,
    }
}

/// "d MMM HH:mm" (macOS `formatHoverTime`), the wall clock of `ms`.
pub fn format_stamp(ms: f64) -> String {
    let total_min = (ms / 60_000.0).floor() as i64;
    let (hour, minute) = ((total_min.div_euclid(60)).rem_euclid(24), total_min.rem_euclid(60));
    let days = total_min.div_euclid(60 * 24);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{day} {} {hour:02}:{minute:02}", MONTHS[(month - 1) as usize])
}

/// "1h 30m" / "45m" (macOS `formatDuration`).
pub fn format_duration(ms: f64) -> String {
    let secs = (ms / 1000.0).max(0.0) as i64;
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// Slint Path commands for dashed vertical rules at the given x positions:
/// a run of short "M x y L x y" strokes (Slint paths have no dash style).
pub fn dashed_vlines(xs: &[f64], top: f64, bottom: f64, dash: f64, gap: f64) -> String {
    let mut out = String::new();
    for &x in xs {
        let mut y = top;
        while y < bottom {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("M {x:.2} {y:.2} L {x:.2} {:.2}", (y + dash).min(bottom)));
            y += dash + gap;
        }
    }
    out
}

/// Slint Path commands for one series: one "M x y" subpath per
/// `core::chart::segments` segment, points joined with "L". Empty -> "".
pub fn series_path(entries: &[Entry], scale: &Scale) -> String {
    let mut out = String::new();
    for seg in segments(entries) {
        for (i, p) in seg.iter().enumerate() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(if i == 0 { "M" } else { "L" });
            out.push_str(&format!(" {:.2} {:.2}", scale.x(p.ms), scale.y(p.value)));
        }
    }
    out
}

/// X ticks `(pixel, label)` and Y gridlines `(value, pixel)`. Labels are
/// rendered after shifting by `local_offset_ms` (a single offset, so a range
/// crossing a DST change may be off by an hour on one side).
pub fn axis_ticks(
    range: Range,
    scale: &Scale,
    max_ticks: usize,
    local_offset_ms: f64,
) -> AxisTicks {
    let span = range.to_ms - range.from_ms;
    let xs = time_ticks(range.from_ms, range.to_ms, max_ticks)
        .into_iter()
        .map(|t| (scale.x(t), format_tick(t + local_offset_ms, span)))
        .collect();
    let ys = Y_TICKS.iter().map(|&v| (v, scale.y(v))).collect();
    (xs, ys)
}

/// Pointer x (px) -> time (ms): inverse of `scale.x`.
pub fn x_to_ms(x_px: f64, scale: &Scale) -> f64 {
    scale.ms_at(x_px)
}

/// Store points (t_unix seconds) -> chart entries (ms).
pub fn points_to_entries(points: &[SeriesPoint]) -> Vec<Entry> {
    points
        .iter()
        .map(|p| Entry { ms: p.t_unix * 1000.0, value: p.utilization })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::chart::make_scale;

    fn scale() -> (Range, Scale) {
        let r = Range { from_ms: 0.0, to_ms: 10.0 * HOUR_MS };
        (r, make_scale(r, 400.0, 200.0, 20.0))
    }
    fn e(h: f64, v: f64) -> Entry {
        Entry { ms: h * HOUR_MS, value: v }
    }

    #[test]
    fn presets() {
        assert_eq!(preset_span_ms("5h"), 5.0 * 3_600_000.0);
        assert_eq!(preset_span_ms("24h"), 86_400_000.0);
        assert_eq!(preset_span_ms("3d"), 3.0 * 86_400_000.0);
        assert_eq!(preset_span_ms("7d"), 7.0 * 86_400_000.0);
        assert_eq!(preset_span_ms("30d"), 30.0 * 86_400_000.0);
    }

    #[test]
    fn interpolation_matches_macos() {
        let es = [e(0.0, 10.0), e(2.0, 50.0)];
        assert_eq!(interpolate(&es, HOUR_MS), Some(30.0));
        assert_eq!(interpolate(&es, 2.0 * HOUR_MS), Some(50.0));
        assert_eq!(interpolate(&es, -HOUR_MS), Some(10.0));
        assert_eq!(interpolate(&es, 9.0 * HOUR_MS), Some(50.0));
        assert_eq!(interpolate(&[], 0.0), None);
    }

    #[test]
    fn stamps_and_durations() {
        // 2026-10-17 13:05 UTC
        let ms = 1_792_242_300_000.0;
        assert_eq!(format_stamp(ms), "17 Oct 13:05");
        assert_eq!(format_stamp(0.0), "1 Jan 00:00");
        assert_eq!(format_duration(90.0 * 60_000.0), "1h 30m");
        assert_eq!(format_duration(45.0 * 60_000.0), "45m");
    }

    #[test]
    fn dashed_rules_are_segments() {
        let p = dashed_vlines(&[10.0, 20.0], 0.0, 20.0, 3.0, 4.0);
        assert_eq!(p.matches('M').count(), 2 * 3); // y = 0, 7, 14 per rule
        assert!(dashed_vlines(&[], 0.0, 20.0, 3.0, 4.0).is_empty());
    }

    #[test]
    fn path_contiguous() {
        let (_, s) = scale();
        let p = series_path(&[e(1.0, 10.0), e(2.0, 20.0)], &s);
        assert!(p.starts_with("M "));
        assert_eq!(p.matches(" L ").count(), 1);
        assert_eq!(p.matches("M ").count(), 1);
    }

    #[test]
    fn path_reset_breaks_line() {
        let (_, s) = scale();
        let p = series_path(&[e(1.0, 50.0), e(2.0, 60.0), e(3.0, 0.0), e(4.0, 5.0)], &s);
        assert_eq!(p.matches("M ").count(), 2, "{p}");
    }

    #[test]
    fn path_empty() {
        let (_, s) = scale();
        assert_eq!(series_path(&[], &s), "");
    }

    #[test]
    fn x_roundtrip() {
        let (_, s) = scale();
        for h in [0.0, 1.5, 5.0, 10.0] {
            let ms = h * HOUR_MS;
            assert!((x_to_ms(s.x(ms), &s) - ms).abs() < 1.0);
        }
    }

    #[test]
    fn ticks() {
        let (r, s) = scale();
        let off = 2.0 * HOUR_MS;
        let (xs, ys) = axis_ticks(r, &s, 5, off);
        assert_eq!(ys.len(), Y_TICKS.len());
        assert_eq!(ys[0], (0.0, s.y(0.0)));
        assert!(!xs.is_empty());
        let t = time_ticks(r.from_ms, r.to_ms, 5)[0];
        assert_eq!(xs[0].0, s.x(t));
        assert_eq!(xs[0].1, format_tick(t + off, 10.0 * HOUR_MS));
        // 00:00 UTC shifted +2h reads 02:00
        let (xs0, _) = axis_ticks(Range { from_ms: 0.0, to_ms: 4.0 * HOUR_MS }, &make_scale(Range { from_ms: 0.0, to_ms: 4.0 * HOUR_MS }, 400.0, 200.0, 20.0), 5, off);
        assert_eq!(xs0[0].1, "02:00");
    }

    #[test]
    fn points_convert() {
        let v = points_to_entries(&[SeriesPoint { t_unix: 2.0, utilization: 7.0, is_limited: false }]);
        assert_eq!(v, vec![Entry { ms: 2000.0, value: 7.0 }]);
    }
}
