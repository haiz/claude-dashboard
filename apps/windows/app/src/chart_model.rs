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

/// Map a range preset to `(window_code, span_ms)`. Window codes: 5h=0, 7d=1,
/// Fable=3. macOS presets carry only a duration; here the short presets (5h,
/// 24h) read the 5-hour window's series and the long ones (3d, 7d, 30d) the
/// 7-day window's. Unknown presets fall back to 24h.
pub fn preset_window_and_span(preset: &str) -> (i64, f64) {
    match preset {
        "5h" => (0, 5.0 * HOUR_MS),
        "3d" => (1, 3.0 * DAY_MS),
        "7d" => (1, 7.0 * DAY_MS),
        "30d" => (1, 30.0 * DAY_MS),
        "fable" => (3, 7.0 * DAY_MS),
        _ => (0, DAY_MS),
    }
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
        assert_eq!(preset_window_and_span("5h"), (0, 5.0 * 3_600_000.0));
        assert_eq!(preset_window_and_span("24h"), (0, 86_400_000.0));
        assert_eq!(preset_window_and_span("3d"), (1, 3.0 * 86_400_000.0));
        assert_eq!(preset_window_and_span("7d"), (1, 7.0 * 86_400_000.0));
        assert_eq!(preset_window_and_span("30d"), (1, 30.0 * 86_400_000.0));
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
        let v = points_to_entries(&[SeriesPoint { t_unix: 2.0, utilization: 7.0 }]);
        assert_eq!(v, vec![Entry { ms: 2000.0, value: 7.0 }]);
    }
}
