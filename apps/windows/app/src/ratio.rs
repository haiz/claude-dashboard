//! X, the 7d cost of a full 5h window (macOS `UsageRatioController`): recomputed at launch
//! and every hour from the usage log into `ratio_logs.db`; the latest value feeds the
//! critical assessment and the Dashboard header button.

use std::sync::Mutex;

use claude_dashboard_core::store::UsageLogStore;
use claude_dashboard_core::usage_ratio::{self, LogRow, RatioPoint, RatioStore, RatioValue};

static CURRENT: Mutex<RatioValue> = Mutex::new(RatioValue::DEFAULT);

/// The X the app uses now.
pub fn current() -> RatioValue {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_current(v: RatioValue) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = v;
}

/// Recomputes X at launch and then every hour (macOS `UsageRatioController.start`), storing
/// each measurement and calling `on_update` with the value the app uses now.
pub fn spawn(on_update: impl Fn(RatioValue) + Send + 'static) {
    std::thread::spawn(move || loop {
        let now = crate::unix_now();
        if let Ok(store) = RatioStore::open() {
            let v = usage_ratio::recompute(&store, |five_from, seven_from| fetch_logs(five_from, seven_from, now), now);
            set_current(v);
            on_update(v);
        }
        std::thread::sleep(std::time::Duration::from_secs(usage_ratio::INTERVAL_S));
    });
}

/// Every account's 5h rows from `five_from` and 7d rows from `seven_from` up to now.
fn fetch_logs(five_from: f64, seven_from: f64, now: f64) -> (Vec<LogRow>, Vec<LogRow>) {
    let Ok(log) = UsageLogStore::open() else { return (Vec::new(), Vec::new()) };
    let rows = |window: i64, from: f64| -> Vec<LogRow> {
        log.series_all(window, from, now + 1.0)
            .into_iter()
            .flat_map(|(id, points)| {
                points.into_iter().map(move |p| LogRow {
                    account_id: id.clone(),
                    t_unix: p.t_unix,
                    utilization: p.utilization,
                    resets_at_unix: p.resets_at_unix,
                })
            })
            .collect()
    };
    (rows(0, five_from), rows(1, seven_from))
}

/// The history for the chart's range (`RANGES` index), oldest first.
pub fn history(range: usize, now: f64) -> Vec<RatioPoint> {
    let from = RANGES.get(range).and_then(|r| r.1).map(|span| now - span);
    RatioStore::open().map(|s| s.series(from, None)).unwrap_or_default()
}

/// The Dashboard header button (macOS `UsageRatioButton.label`).
pub fn label(v: &RatioValue) -> String {
    if v.is_default() {
        format!("X {}% (default)", v.x.round() as i64)
    } else {
        format!("X {:.1}%", v.x)
    }
}

/// Its tooltip (macOS `UsageRatioButton.help`); the time is local.
pub fn help(v: &RatioValue, local_offset_s: f64) -> String {
    let head = format!("100% of a 5h window costs about {:.1}% of the 7d quota.", v.x);
    match v.updated_unix {
        None => format!("{head} Default value: not enough usage history yet. Click for the history."),
        Some(t) => format!(
            "{head} Measured over the last 3 days, updated hourly (last at {}). Click for the history.",
            crate::ping::clock(t, local_offset_s)
        ),
    }
}

/// The chart range picker: 7 days, 30 days, All (90 days kept).
pub const RANGES: [(&str, Option<f64>); 3] = [("7 days", Some(7.0 * 86_400.0)), ("30 days", Some(30.0 * 86_400.0)), ("All", None)];

/// The chart in a 1000 x 1000 view box: the line, gridlines and labels as fractions of the
/// plot (0 = left/bottom).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartModel {
    pub path: String,
    pub y_ticks: Vec<(f64, String)>,
    pub x_ticks: Vec<(f64, String)>,
    pub t_from: f64,
    pub t_to: f64,
    pub y_max: f64,
}

pub fn chart(points: &[RatioPoint], local_offset_s: f64) -> ChartModel {
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return ChartModel::default() };
    let t_from = first.t_unix;
    // A single point still gets an hour of span, so it sits at the left edge, not on a zero width.
    let t_to = if last.t_unix > t_from { last.t_unix } else { t_from + 3600.0 };
    let max = points.iter().map(|p| p.x).fold(0.0, f64::max);
    let step = nice_step(max / 4.0);
    let y_max = ((max / step).ceil() * step).max(step);
    let px = |t: f64| (t - t_from) / (t_to - t_from) * 1000.0;
    let py = |x: f64| 1000.0 - x / y_max * 1000.0;
    let path = points
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{} {} {}", if i == 0 { "M" } else { "L" }, fmt(px(p.t_unix)), fmt(py(p.x))))
        .collect::<Vec<_>>()
        .join(" ");
    let mut y_ticks = Vec::new();
    let mut v = 0.0;
    while v <= y_max + step / 2.0 {
        y_ticks.push((v / y_max, fmt(v)));
        v += step;
    }
    let (from_ms, to_ms) = ((t_from + local_offset_s) * 1000.0, (t_to + local_offset_s) * 1000.0);
    let x_ticks = claude_dashboard_core::chart::time_ticks(from_ms, to_ms, 5)
        .into_iter()
        .map(|ms| ((ms - from_ms) / (to_ms - from_ms), claude_dashboard_core::chart::format_tick(ms, to_ms - from_ms)))
        .collect();
    ChartModel { path, y_ticks, x_ticks, t_from, t_to, y_max }
}

/// The smallest of 1, 2, 2.5, 5 x 10^k at or above `raw`.
fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag)
}

/// A number without trailing zeros: 20 -> "20", 2.5 -> "2.5", 333.333 -> "333.3".
fn fmt(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r:.1}") }
}

/// The point nearest `t` (macOS `UsageRatioChart.nearest`).
pub fn nearest(points: &[RatioPoint], t: f64) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1.t_unix - t).abs().total_cmp(&(b.1.t_unix - t).abs()))
        .map(|(i, _)| i)
}

/// The hover readout under the chart: "Oct 17, 2026 at 10:44 AM: X 16.3%".
pub fn readout(p: &RatioPoint, local_offset_s: f64) -> String {
    format!("{}: X {:.1}%", crate::ping_log_view::abbreviated_date_time(p.t_unix + local_offset_s), p.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1_800_000_000.0;

    #[test]
    fn label_measured_and_default() {
        assert_eq!(label(&RatioValue { x: 16.27, updated_unix: Some(T0) }), "X 16.3%");
        assert_eq!(label(&RatioValue::DEFAULT), "X 16% (default)");
    }

    #[test]
    fn help_names_the_value_and_the_update_time() {
        // 1_000_000 s = 13:46:40 UTC.
        let h = help(&RatioValue { x: 16.27, updated_unix: Some(1_000_000.0) }, 0.0);
        assert_eq!(h, "100% of a 5h window costs about 16.3% of the 7d quota. Measured over the last 3 days, updated hourly (last at 1:46 PM). Click for the history.");
        let d = help(&RatioValue::DEFAULT, 0.0);
        assert_eq!(d, "100% of a 5h window costs about 16.0% of the 7d quota. Default value: not enough usage history yet. Click for the history.");
    }

    fn p(dt: f64, x: f64) -> RatioPoint {
        RatioPoint { t_unix: T0 + dt, x, sum_five_hour: 0.0, sum_seven_day: 0.0 }
    }

    #[test]
    fn nearest_point() {
        let points = [p(0.0, 0.0), p(3600.0, 1.0), p(7200.0, 2.0)];
        assert_eq!(nearest(&points, T0 + 4000.0), Some(1));
        assert_eq!(nearest(&[], T0), None);
    }

    #[test]
    fn chart_scales_from_zero_and_draws_the_line() {
        let m = chart(&[p(0.0, 15.0), p(3600.0, 17.0)], 0.0);
        assert_eq!((m.t_from, m.t_to), (T0, T0 + 3600.0));
        assert_eq!(m.y_max, 20.0, "a nice maximum above 17");
        assert_eq!(m.path, "M 0 1000 L 1000 1000".replace("1000 L", "250 L").replace("L 1000 1000", "L 1000 150"));
        assert_eq!(m.y_ticks.first(), Some(&(0.0, "0".to_string())));
        assert_eq!(m.y_ticks.last(), Some(&(1.0, "20".to_string())));
        assert!(!m.x_ticks.is_empty());
        let one = chart(&[p(0.0, 16.0)], 0.0);
        assert!(one.t_to > one.t_from, "a single point still gets a time span");
        assert_eq!(chart(&[], 0.0), ChartModel::default());
    }

    #[test]
    fn readout_format() {
        // 2026-10-17 03:44:03 UTC
        let r = RatioPoint { t_unix: 1_792_208_643.0, x: 16.27, sum_five_hour: 0.0, sum_seven_day: 0.0 };
        assert_eq!(readout(&r, 7.0 * 3600.0), "Oct 17, 2026 at 10:44 AM: X 16.3%");
    }
}
