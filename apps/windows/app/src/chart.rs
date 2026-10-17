//! The reusable chart container: preset / range / pan / zoom / ruler / hover
//! state and the view it produces (`Interaction`, plain data, testable without
//! Slint). The per-account History chart (`history.rs`) and the Overview
//! (`overview.rs`) both wrap it. All geometry comes from `core::chart` via
//! `chart_model`; the `.slint` only draws what this module computes.

use claude_dashboard_core::chart::{
    average_rate, make_scale, noon_markers, rate_at, Entry, Range, Scale, Y_MAX,
};
use claude_dashboard_core::store::ResetCycle;

use crate::chart_model::{
    axis_ticks, dashed_hlines, dashed_vlines, format_duration, format_stamp, interpolate,
    preset_span_ms, series_path, tip_on_left, x_to_ms,
};
use crate::{ChartState, ChartTick, ChartTipRow};

/// Space (px) around the plot frame. As macOS: no frame border, the plot runs
/// to the left edge and the y labels sit on the right; x labels go below.
pub(crate) const PAD_LEFT: f64 = 2.0;
pub(crate) const PAD_TOP: f64 = 4.0;
pub(crate) const PAD_RIGHT: f64 = 40.0;
pub(crate) const PAD_BOTTOM: f64 = 20.0;
/// macOS `.chartYScale(domain: 0...105)`: headroom so 100% is not the top edge.
const Y_DOMAIN_MAX: f64 = 105.0;
pub const DEFAULT_PRESET: &str = "24h";
/// The smallest zoom span (macOS `InteractiveChartContainer`: 5 minutes).
const MIN_ZOOM_MS: f64 = 5.0 * 60_000.0;

/// Drag behaviour on the plot (macOS `InteractionMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Pan,
    Zoom,
}

/// One line of the hover card: the History card has a single unnamed row;
/// the Overview card one per shown account, with its palette slot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TipRow {
    pub name: String,
    pub slot: i32,
    pub value: Option<f64>,
    pub rate: Option<f64>,
}

/// The hover card (macOS `hoverTooltip`): the time, then the rows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tip {
    pub time: String,
    pub rows: Vec<TipRow>,
}

/// What the UI should show; plain data so it is testable without Slint.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    pub line_path: String,
    pub x_ticks: Vec<(f64, String)>,
    pub y_ticks: Vec<(f64, String)>,
    /// Dashed vertical gridlines at the x ticks (macOS `AxisGridLine`).
    pub grid_path: String,
    /// The pointer (px) while it is over the plot.
    pub cross: Option<(f64, f64)>,
    /// Dashed hover rule, plus the ruler's horizontal crosshair while armed.
    pub cross_path: String,
    pub tip: Option<Tip>,
    /// The hover / measure card sits top-left (pointer on the right half).
    pub tip_left: bool,
    pub empty: bool,
    pub loading: bool,
    pub preset: String,
    pub plot: (f64, f64, f64, f64),
    /// "Custom" label shown after a pan / zoom / cycle selection.
    pub custom: bool,
    pub pan_mode: bool,
    pub ruler: bool,
    /// Dashed local-noon rules as one Slint path.
    pub noon_path: String,
    /// "Avg: 1.2%/h" or "Avg: --" and whether it is shown hot (> 0.1).
    pub avg_text: String,
    pub avg_hot: bool,
    /// Ruler: x of point A / B (px) and the Δ% card lines.
    pub measure_a_x: Option<f64>,
    pub measure_b_x: Option<f64>,
    pub measure_range: String,
    pub measure_delta: String,
    pub measure_duration: String,
    pub measure_up: bool,
}

#[derive(Debug, Clone)]
pub struct Interaction {
    /// The active preset; stays set while `custom` (the label is drawn instead).
    pub preset: String,
    /// Usage window code: 5h = 0, 7d = 1, Fable = 3.
    pub window: i64,
    pub mode: Mode,
    /// A pan / zoom / cycle selection froze the range (no live-edge following).
    pub custom: bool,
    /// The ruler tool is armed: clicks measure, drags and presets are off.
    pub ruler: bool,
    /// Ruler points: A, then B once the second click lands.
    pub measure: Option<(Entry, Option<Entry>)>,
    pub selected_cycle: Option<ResetCycle>,
    entries: Vec<Entry>,
    pub range: Range,
    size: (f64, f64),
    hover: Option<(f64, f64)>,
    /// The last pointer x: picks the card corner, and like macOS the measure
    /// card keeps it after the pointer leaves.
    last_hover_x: f64,
    loading: bool,
    now_ms: f64,
}

impl Interaction {
    pub fn new(preset: &str, now_ms: f64) -> Self {
        Self {
            preset: preset.to_string(),
            window: 0,
            mode: Mode::Pan,
            custom: false,
            ruler: false,
            measure: None,
            selected_cycle: None,
            entries: Vec::new(),
            range: preset_range(preset, now_ms),
            size: (640.0, 320.0),
            hover: None,
            last_hover_x: 0.0,
            loading: true,
            now_ms,
        }
    }

    /// Switch the usage window (5h / 7d / Fable): drops the cycle filter and
    /// the ruler points; the range and its `custom` state are kept.
    pub fn set_window(&mut self, window: i64) {
        self.window = window;
        self.selected_cycle = None;
        self.measure = None;
    }

    /// A preset click: the trailing window of that preset ending at `now_ms`;
    /// clears `custom` and any cycle filter. Entries stay (the caller loaded
    /// the whole window's history).
    pub fn set_preset(&mut self, preset: &str, now_ms: f64) {
        self.preset = preset.to_string();
        self.now_ms = now_ms;
        self.range = preset_range(preset, now_ms);
        self.custom = false;
        self.selected_cycle = None;
        self.measure = None;
        self.hover = None;
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Arm / disarm the ruler; disarming drops the points.
    pub fn set_ruler(&mut self, on: bool) {
        self.ruler = on;
        if !on {
            self.measure = None;
        }
    }

    /// Pan by a pointer movement in px (drag right = back in time); freezes
    /// the range (`custom`) and never moves its end past "now".
    pub fn pan_by_px(&mut self, dx: f64) {
        let sc = self.scale();
        let span = self.range.to_ms - self.range.from_ms;
        if sc.plot_width <= 0.0 || span <= 0.0 {
            return;
        }
        let delta = -dx * span / sc.plot_width;
        let mut from = self.range.from_ms + delta;
        let mut to = self.range.to_ms + delta;
        if to > self.now_ms {
            from -= to - self.now_ms;
            to = self.now_ms;
        }
        self.range = Range {
            from_ms: from,
            to_ms: to,
        };
        self.custom = true;
        self.hover = None;
    }

    /// Zoom onto a drag-selected px span: clamped to the visible range;
    /// selections under 5 minutes are ignored. Freezes the range.
    pub fn zoom_select(&mut self, x0: f64, x1: f64) {
        let sc = self.scale();
        let (a, b) = (x_to_ms(x0.min(x1), &sc), x_to_ms(x0.max(x1), &sc));
        let a = a.max(self.range.from_ms);
        let b = b.min(self.range.to_ms);
        if b - a < MIN_ZOOM_MS {
            return;
        }
        self.range = Range {
            from_ms: a,
            to_ms: b,
        };
        self.custom = true;
        self.hover = None;
    }

    /// Ruler click: first click sets A, second sets B (ignored within a
    /// second of A), a third starts over. Values are interpolated on the
    /// series like macOS; clicks outside the plot are ignored.
    pub fn measure_click(&mut self, x: f64, _y: f64) {
        let sc = self.scale();
        if x < sc.left || x > sc.right {
            return;
        }
        let ms = x_to_ms(x, &sc);
        let Some(value) = interpolate(&self.entries, ms) else {
            return;
        };
        let p = Entry { ms, value };
        self.measure = match self.measure {
            None => Some((p, None)),
            Some((a, None)) => {
                if (p.ms - a.ms).abs() > 1000.0 {
                    Some((a, Some(p)))
                } else {
                    Some((a, None))
                }
            }
            Some((_, Some(_))) => None,
        };
    }

    /// Filter the chart to one reset cycle (range = the cycle's data extent
    /// plus a 2% margin, at least 5 minutes), or `None` for "Show All": back
    /// to the preset's trailing window.
    pub fn select_cycle(&mut self, cycle: Option<ResetCycle>) {
        self.measure = None;
        self.hover = None;
        match cycle {
            Some(c) => {
                let (first, last) = (c.first_unix * 1000.0, c.last_unix * 1000.0);
                let span = (last - first).max(MIN_ZOOM_MS);
                let margin = span * 0.02;
                self.range = Range {
                    from_ms: first - margin,
                    to_ms: first + span + margin,
                };
                self.custom = true;
                self.selected_cycle = Some(c);
            }
            None => {
                if self.selected_cycle.take().is_some() {
                    self.range = preset_range(&self.preset, self.now_ms);
                    self.custom = false;
                }
            }
        }
    }

    /// A refresh finished: while following the live edge (a preset, no
    /// pan / zoom / cycle) slide the window to end at `now_ms`. Returns
    /// whether the range moved. The armed ruler is left alone.
    pub fn live_tick(&mut self, now_ms: f64) -> bool {
        self.now_ms = now_ms;
        if self.ruler || self.custom || self.selected_cycle.is_some() {
            return false;
        }
        self.range = preset_range(&self.preset, now_ms);
        true
    }

    pub(crate) fn scale(&self) -> Scale {
        plot_scale(self.range, self.size.0, self.size.1)
    }

    /// The loaded series (ascending).
    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn set_size(&mut self, w: f64, h: f64) {
        if w > 0.0 && h > 0.0 {
            self.size = (w, h);
        }
    }

    /// New series. A following (non-custom, no cycle) range re-anchors on
    /// `now_ms`; a frozen one keeps its range.
    pub fn loaded(&mut self, entries: Vec<Entry>, now_ms: f64) {
        self.now_ms = now_ms;
        if !self.custom && self.selected_cycle.is_none() {
            self.range = preset_range(&self.preset, now_ms);
        }
        self.entries = entries;
        self.hover = None;
        self.loading = false;
    }

    /// The window changed: show "loading" with no data until `loaded` lands
    /// (the range and its custom state are kept).
    pub(crate) fn clear_for_reload(&mut self) {
        self.entries.clear();
        self.hover = None;
        self.loading = true;
    }

    pub fn hover_at(&mut self, x: f64, y: f64) {
        self.hover = Some((x, y));
        self.last_hover_x = x;
    }

    pub fn hover_off(&mut self) {
        self.hover = None;
    }

    /// Drag-select between two pointer x positions (px), see `zoom_select`.
    pub fn select(&mut self, x0: f64, x1: f64) {
        self.zoom_select(x0, x1);
    }

    /// Double-click: back to the preset's trailing window (frees `custom`
    /// and any cycle filter).
    pub fn reset(&mut self) {
        // While measuring the range must hold still (macOS disables interaction).
        if self.ruler {
            return;
        }
        self.range = preset_range(&self.preset, self.now_ms);
        self.custom = false;
        self.selected_cycle = None;
        self.hover = None;
    }

    pub fn view(&self, local_offset_ms: f64) -> View {
        let sc = self.scale();
        let max_ticks = ((sc.plot_width / 90.0) as usize).max(2);
        let (xs, ys) = axis_ticks(self.range, &sc, max_ticks, local_offset_ms);
        let empty = self.entries.is_empty();
        let top = frame_top(&sc);
        let mut cross = None;
        let mut cross_path = String::new();
        let mut tip = None;
        // As macOS: a dashed rule follows the pointer; the card shows the time,
        // the interpolated value and the local burn rate.
        if let Some((hx, hy)) = self.hover {
            let inside = hx >= sc.left && hx <= sc.right && hy >= top && hy <= sc.bottom;
            if inside {
                let ms = x_to_ms(hx, &sc);
                cross = Some((hx, hy));
                cross_path = dashed_vlines(&[hx], top, sc.bottom, 4.0, 4.0);
                // Ruler armed, no B yet: a horizontal crosshair at the pointer.
                if self.ruler && !matches!(self.measure, Some((_, Some(_)))) {
                    cross_path.push(' ');
                    cross_path.push_str(&dashed_hlines(&[hy], sc.left, sc.right, 4.0, 4.0));
                }
                if let Some(value) = interpolate(&self.entries, ms) {
                    tip = Some(Tip {
                        time: format_stamp(ms + local_offset_ms),
                        rows: vec![TipRow {
                            name: String::new(),
                            slot: 0,
                            value: Some(value),
                            rate: rate_at(&self.entries, ms),
                        }],
                    });
                }
            }
        }
        let grid_x: Vec<f64> = xs.iter().map(|t| t.0).collect();
        let noon_x: Vec<f64> = noon_markers(self.range.from_ms, self.range.to_ms, local_offset_ms)
            .into_iter()
            .map(|ms| sc.x(ms))
            .collect();
        let (avg_text, avg_hot) =
            match average_rate(&self.entries, self.range.from_ms, self.range.to_ms) {
                Some(r) => (format!("Avg: {r:.1}%/h"), r > 0.1),
                None => ("Avg: --".to_string(), false),
            };
        let mut v = View {
            line_path: series_path(&vis_with_context(&self.entries, self.range), &sc),
            x_ticks: xs,
            y_ticks: ys
                .into_iter()
                .map(|(v, py)| (py, format!("{v:.0}%")))
                .collect(),
            grid_path: dashed_vlines(&grid_x, top, sc.bottom, 2.0, 3.0),
            cross,
            cross_path,
            tip,
            tip_left: tip_on_left(self.last_hover_x, sc.left, sc.right),
            empty,
            loading: self.loading,
            preset: self.preset.clone(),
            plot: (sc.left, top, sc.plot_width, sc.bottom - top),
            custom: self.custom,
            pan_mode: self.mode == Mode::Pan,
            ruler: self.ruler,
            noon_path: dashed_vlines(&noon_x, top, sc.bottom, 3.0, 4.0),
            avg_text,
            avg_hot,
            ..View::default()
        };
        if let Some((a, b)) = self.measure {
            v.measure_a_x = Some(sc.x(a.ms));
            if let Some(b) = b {
                // As macOS: the hover rule and card step aside while the Δ card shows.
                v.cross = None;
                v.cross_path.clear();
                v.tip = None;
                v.measure_b_x = Some(sc.x(b.ms));
                let (s, e) = if a.ms <= b.ms { (a, b) } else { (b, a) };
                let delta = e.value - s.value;
                v.measure_range = format!(
                    "{} \u{2192} {}",
                    format_stamp(s.ms + local_offset_ms),
                    format_stamp(e.ms + local_offset_ms)
                );
                v.measure_delta = format!("{delta:+.0}%: {:.0}% \u{2192} {:.0}%", s.value, e.value);
                v.measure_duration = format_duration(e.ms - s.ms);
                v.measure_up = delta >= 0.0;
            }
        }
        v
    }
}

/// Entries inside the range plus one neighbour on each side, so the line
/// enters and leaves the plot edge instead of starting mid-air when zoomed
/// (the .slint clips the path to the plot frame).
pub(crate) fn vis_with_context(entries: &[Entry], r: Range) -> Vec<Entry> {
    let first = entries.iter().position(|e| e.ms >= r.from_ms);
    let last = entries.iter().rposition(|e| e.ms <= r.to_ms);
    match (first, last) {
        (Some(f), Some(l)) if f <= l => {
            let f = f.saturating_sub(1);
            let l = (l + 1).min(entries.len() - 1);
            entries[f..=l].to_vec()
        }
        _ => Vec::new(),
    }
}

/// The value scale of a `w` x `h` chart: the frame inside the PAD_* margins,
/// with 0% at its bottom and `Y_DOMAIN_MAX` at its top, so `scale.top` is the
/// 100% line and `frame_top` the frame's real top.
pub(crate) fn plot_scale(range: Range, w: f64, h: f64) -> Scale {
    let fw = (w - PAD_LEFT - PAD_RIGHT).max(1.0);
    let fh = (h - PAD_TOP - PAD_BOTTOM).max(1.0);
    let head = fh * (Y_DOMAIN_MAX - Y_MAX) / Y_DOMAIN_MAX;
    let mut sc = make_scale(range, fw, fh - head, 0.0);
    sc.left += PAD_LEFT;
    sc.right += PAD_LEFT;
    sc.top += PAD_TOP + head;
    sc.bottom += PAD_TOP + head;
    sc
}

/// The plot frame's top (px): above the 100% line by the y-domain headroom.
pub(crate) fn frame_top(sc: &Scale) -> f64 {
    sc.top - sc.plot_height * (Y_DOMAIN_MAX - Y_MAX) / Y_MAX
}

pub(crate) fn preset_range(preset: &str, now_ms: f64) -> Range {
    Range {
        from_ms: now_ms - preset_span_ms(preset),
        to_ms: now_ms,
    }
}

pub(crate) fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

pub(crate) fn offset_ms() -> f64 {
    crate::model::local_offset_s() * 1000.0
}

pub(crate) fn to_state(v: &View) -> ChartState {
    let ticks = |t: &[(f64, String)]| -> slint::ModelRc<ChartTick> {
        let items: Vec<ChartTick> = t
            .iter()
            .map(|(p, l)| ChartTick {
                pos: *p as f32,
                label: l.as_str().into(),
            })
            .collect();
        slint::ModelRc::new(slint::VecModel::from(items))
    };
    let tip = v.tip.clone().unwrap_or_default();
    let tip_rows: Vec<ChartTipRow> = tip
        .rows
        .iter()
        .map(|r| ChartTipRow {
            name: r.name.as_str().into(),
            slot: r.slot,
            value: r
                .value
                .map(|u| format!("{u:.0}%"))
                .unwrap_or_default()
                .into(),
            rate: r
                .rate
                .map(|x| format!("{x:+.1}%/h"))
                .unwrap_or_else(|| "--".to_string())
                .into(),
            has_rate: r.rate.is_some(),
            rate_up: r.rate.is_some_and(|x| x > 0.0),
        })
        .collect();
    ChartState {
        line_path: v.line_path.as_str().into(),
        x_ticks: ticks(&v.x_ticks),
        y_ticks: ticks(&v.y_ticks),
        grid_path: v.grid_path.as_str().into(),
        cross_visible: v.cross.is_some(),
        cross_path: v.cross_path.as_str().into(),
        tip_visible: v.tip.is_some(),
        tip_left: v.tip_left,
        tip_time: tip.time.as_str().into(),
        tip_rows: slint::ModelRc::new(slint::VecModel::from(tip_rows)),
        empty: v.empty,
        loading: v.loading,
        preset: v.preset.as_str().into(),
        plot_left: v.plot.0 as f32,
        plot_top: v.plot.1 as f32,
        plot_width: v.plot.2 as f32,
        plot_height: v.plot.3 as f32,
        custom: v.custom,
        pan_mode: v.pan_mode,
        ruler: v.ruler,
        noon_path: v.noon_path.as_str().into(),
        avg_text: v.avg_text.as_str().into(),
        avg_hot: v.avg_hot,
        measure_a_visible: v.measure_a_x.is_some(),
        measure_a_x: v.measure_a_x.unwrap_or(0.0) as f32,
        measure_b_visible: v.measure_b_x.is_some(),
        measure_b_x: v.measure_b_x.unwrap_or(0.0) as f32,
        measure_range: v.measure_range.as_str().into(),
        measure_delta: v.measure_delta.as_str().into(),
        measure_duration: v.measure_duration.as_str().into(),
        measure_up: v.measure_up,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::store::ResetCycle;

    const H: f64 = 3_600_000.0;
    const NOW: f64 = 1_000.0 * 86_400_000.0;

    fn loaded() -> Interaction {
        let mut i = Interaction::new("5h", NOW);
        i.set_size(536.0, 336.0); // frame 494 x 312 inside the PAD_* margins
        i.loaded(
            vec![
                Entry {
                    ms: NOW - 4.0 * H,
                    value: 10.0,
                },
                Entry {
                    ms: NOW - 2.0 * H,
                    value: 40.0,
                },
                Entry {
                    ms: NOW - 1.0 * H,
                    value: 60.0,
                },
            ],
            NOW,
        );
        i
    }

    #[test]
    fn empty_series_has_no_crosshair() {
        let mut i = Interaction::new("5h", NOW);
        i.loaded(Vec::new(), NOW);
        i.hover_at(200.0, 100.0);
        let v = i.view(0.0);
        assert!(v.empty && v.tip.is_none() && v.line_path.is_empty());
    }

    #[test]
    fn hover_tooltip_is_time_interpolated_value_and_rate() {
        let mut i = loaded();
        let sc = i.scale();
        let hx = sc.x(NOW - 2.1 * H);
        i.hover_at(hx, sc.top + 10.0);
        let v = i.view(0.0);
        let (cx, _) = v.cross.expect("crosshair");
        // The dashed rule follows the pointer.
        assert!((cx - hx).abs() < 1e-6);
        assert!(
            v.cross_path.starts_with(&format!("M {hx:.2} ")),
            "{}",
            v.cross_path
        );
        // 10 -> 40 over 2h = +15%/h.
        let tip = v.tip.expect("card");
        assert_eq!(tip.time, "26 Sep 21:54");
        assert_eq!(tip.rows.len(), 1);
        assert!((tip.rows[0].value.unwrap() - 38.5).abs() < 1e-6);
        assert_eq!(tip.rows[0].rate, Some(15.0));
        // Pointer on the right half (2.9h into 5h): the card is pinned top-left,
        // and top-right for a pointer on the left half.
        assert!(v.tip_left);
        i.hover_at(sc.left + 5.0, sc.top + 10.0);
        assert!(!i.view(0.0).tip_left);
    }

    #[test]
    fn y_domain_has_headroom_above_100() {
        let i = loaded();
        let sc = i.scale();
        let v = i.view(0.0);
        let (_, top, _, height) = v.plot;
        assert_eq!(top, PAD_TOP);
        assert!((top + height - (336.0 - PAD_BOTTOM)).abs() < 1e-9);
        // 100% sits 5/105 of the frame below its top; 0% on the bottom.
        assert!((sc.y(100.0) - (top + height * 5.0 / 105.0)).abs() < 1e-9);
        assert!((sc.y(0.0) - (top + height)).abs() < 1e-9);
        assert!((frame_top(&sc) - top).abs() < 1e-9);
        // The hover reaches into the headroom.
        let mut i = i;
        i.hover_at(sc.x(NOW - 2.0 * H), top + 1.0);
        assert!(i.view(0.0).cross.is_some());
    }

    #[test]
    fn ruler_adds_horizontal_crosshair_until_b() {
        let mut i = loaded();
        let sc = i.scale();
        let (hx, hy) = (sc.x(NOW - 2.0 * H), sc.y(50.0));
        i.hover_at(hx, hy);
        // Horizontal dashes all start at y = hy (a vertical one may, once).
        let rows = |p: &str| p.matches(&format!(" {hy:.2} L ")).count();
        assert!(rows(&i.view(0.0).cross_path) <= 1);
        i.set_ruler(true);
        assert!(rows(&i.view(0.0).cross_path) > 1);
        i.measure_click(sc.x(NOW - 4.0 * H), hy);
        assert!(rows(&i.view(0.0).cross_path) > 1, "still armed with only A");
        i.measure_click(sc.x(NOW - 1.0 * H), hy);
        let v = i.view(0.0);
        assert!(v.cross_path.is_empty() && v.tip.is_none() && v.measure_b_x.is_some());
    }

    #[test]
    fn reset_is_ignored_while_measuring() {
        let mut i = loaded();
        i.pan_by_px(40.0);
        let panned = i.range;
        i.set_ruler(true);
        i.reset();
        assert_eq!(i.range, panned);
        i.set_ruler(false);
        i.reset();
        assert_ne!(i.range, panned);
    }

    #[test]
    fn hover_off_plot_hides_crosshair() {
        let mut i = loaded();
        i.hover_at(1.0, 1.0); // inside padding, outside plot
        assert!(i.view(0.0).cross.is_none());
        let sc = i.scale();
        i.hover_at(sc.right + 1.0, sc.top + 5.0);
        assert!(i.view(0.0).cross.is_none());
        i.hover_at(sc.x(NOW - 2.0 * H), sc.top + 5.0);
        assert!(i.view(0.0).cross.is_some());
        i.hover_off();
        assert!(i.view(0.0).cross.is_none());
    }

    #[test]
    fn drag_selects_subrange_then_reset_restores() {
        let mut i = loaded();
        let base = i.range;
        let sc = i.scale();
        i.select(sc.x(NOW - 3.0 * H), sc.x(NOW - 1.5 * H));
        assert!(i.range.from_ms > base.from_ms && i.range.to_ms < base.to_ms);
        assert!(i.range.from_ms < i.range.to_ms);
        assert!((i.range.to_ms - i.range.from_ms - 1.5 * H).abs() < 1000.0);
        i.reset();
        assert_eq!(i.range, base);
    }

    #[test]
    fn drag_is_clamped_and_order_independent() {
        let mut i = loaded();
        let base = i.range;
        let sc = i.scale();
        i.select(sc.right + 500.0, sc.left - 500.0);
        assert_eq!(i.range, base);
        // Zero-width selection is ignored.
        i.select(100.0, 100.0);
        assert_eq!(i.range, base);
    }

    #[test]
    fn zoom_hides_out_of_range_hover_targets() {
        let mut i = loaded();
        let sc = i.scale();
        i.select(sc.x(NOW - 2.5 * H), sc.x(NOW - 0.5 * H));
        let sc2 = i.scale();
        i.hover_at(sc2.left + 1.0, sc2.top + 1.0);
        let (cx, _) = i.view(0.0).cross.expect("crosshair");
        assert!(cx >= sc2.left && cx <= sc2.right);
    }

    // ---- Task 10: container state (presets, custom, pan/zoom, ruler, cycles) ----

    fn cycle(first_h: f64, last_h: f64) -> ResetCycle {
        ResetCycle {
            resets_at_unix: (NOW + last_h * H + 600_000.0) / 1000.0,
            first_unix: (NOW + first_h * H) / 1000.0,
            last_unix: (NOW + last_h * H) / 1000.0,
            peak: 60.0,
            points: 3,
        }
    }

    #[test]
    fn default_preset_is_24h_on_5h_window() {
        let i = Interaction::new(DEFAULT_PRESET, NOW);
        assert_eq!(i.preset, "24h");
        assert_eq!(i.window, 0);
        assert!(!i.custom && i.mode == Mode::Pan && !i.ruler);
        assert_eq!(i.range.to_ms - i.range.from_ms, 24.0 * H);
        assert_eq!(i.range.to_ms, NOW);
    }

    #[test]
    fn zoom_never_below_five_minutes() {
        let mut i = loaded();
        let sc = i.scale();
        // A selection narrower than 5 minutes is ignored.
        let before = i.range;
        i.select(sc.x(NOW - 2.0 * H), sc.x(NOW - 2.0 * H + 60_000.0));
        assert_eq!(i.range, before);
        // Zoom in repeatedly: the span never drops under 5 minutes.
        for _ in 0..12 {
            let sc = i.scale();
            let mid = (sc.left + sc.right) / 2.0;
            i.zoom_select(mid - 20.0, mid + 20.0);
            assert!(i.range.to_ms - i.range.from_ms >= 5.0 * 60_000.0 - 1e-6);
        }
        assert!(i.custom);
    }

    #[test]
    fn pan_sets_custom_and_stops_following_live_edge() {
        let mut i = loaded();
        let span = i.range.to_ms - i.range.from_ms;
        i.pan_by_px(50.0);
        assert!(i.custom);
        // Drag right = back in time, by 50 px of the plot's ms-per-px.
        let expect = 50.0 * span / i.scale().plot_width;
        assert!(
            (i.range.from_ms - (NOW - span - expect)).abs() < 1.0,
            "{:?}",
            i.range
        );
        let frozen = i.range;
        assert!(!i.live_tick(NOW + H));
        assert_eq!(i.range, frozen);
    }

    #[test]
    fn pan_cannot_pass_now() {
        let mut i = loaded();
        i.pan_by_px(-500.0);
        assert_eq!(i.range.to_ms, NOW);
        assert_eq!(i.range.to_ms - i.range.from_ms, 5.0 * H);
    }

    #[test]
    fn live_tick_follows_now_while_not_custom() {
        let mut i = loaded();
        assert!(i.live_tick(NOW + H));
        assert_eq!(i.range.to_ms, NOW + H);
        assert_eq!(i.range.to_ms - i.range.from_ms, 5.0 * H);
    }

    #[test]
    fn preset_click_clears_custom() {
        let mut i = loaded();
        i.pan_by_px(30.0);
        assert!(i.custom);
        i.set_preset("24h", NOW + H);
        assert!(!i.custom);
        assert_eq!(i.preset, "24h");
        assert_eq!(i.range.to_ms, NOW + H);
        assert_eq!(i.range.to_ms - i.range.from_ms, 24.0 * H);
    }

    #[test]
    fn measure_two_clicks_gives_delta() {
        let mut i = loaded();
        let sc = i.scale();
        i.measure_click(sc.x(NOW - 4.0 * H), sc.top + 5.0);
        let (a, b) = i.measure.expect("point A");
        assert!(b.is_none() && (a.value - 10.0).abs() < 1e-6);
        i.measure_click(sc.x(NOW - 2.0 * H), sc.top + 5.0);
        let (a, b) = i.measure.expect("both");
        let b = b.expect("point B");
        assert!((b.value - a.value - 30.0).abs() < 1e-6);
        let v = i.view(0.0);
        assert!(v.measure_delta.starts_with("+30%"), "{}", v.measure_delta);
        assert_eq!(v.measure_duration, "2h 0m");
    }

    #[test]
    fn measure_third_click_restarts() {
        let mut i = loaded();
        let sc = i.scale();
        i.measure_click(sc.x(NOW - 4.0 * H), 0.0);
        i.measure_click(sc.x(NOW - 2.0 * H), 0.0);
        i.measure_click(sc.x(NOW - 1.0 * H), 0.0);
        assert!(i.measure.is_none());
        i.measure_click(sc.x(NOW - 3.0 * H), 0.0);
        assert!(matches!(i.measure, Some((_, None))));
    }

    #[test]
    fn selecting_cycle_sets_range_to_cycle() {
        let mut i = loaded();
        let c = cycle(-4.0, -1.0);
        i.select_cycle(Some(c));
        assert_eq!(i.selected_cycle, Some(c));
        assert!(i.custom);
        assert!(i.range.from_ms <= c.first_unix * 1000.0 && i.range.to_ms >= c.last_unix * 1000.0);
        assert!(i.range.to_ms - i.range.from_ms < 4.0 * H);
        // Frozen: the live edge does not move it.
        let frozen = i.range;
        i.live_tick(NOW + H);
        assert_eq!(i.range, frozen);
        // Show All restores the preset window.
        i.select_cycle(None);
        assert!(i.selected_cycle.is_none() && !i.custom);
        assert_eq!(i.range.to_ms - i.range.from_ms, 5.0 * H);
    }

    #[test]
    fn set_window_clears_cycle() {
        let mut i = loaded();
        i.select_cycle(Some(cycle(-4.0, -1.0)));
        let sc = i.scale();
        i.measure_click(sc.x(NOW - 2.0 * H), 0.0);
        i.set_window(1);
        assert_eq!(i.window, 1);
        assert!(i.selected_cycle.is_none() && i.measure.is_none());
    }

    #[test]
    fn ruler_off_clears_measure_and_view_has_noon_and_avg() {
        let mut i = loaded();
        i.set_preset("24h", NOW);
        let v = i.view(0.0);
        assert!(v.avg_text.starts_with("Avg: "), "{}", v.avg_text);
        assert!(!v.noon_path.is_empty());
        let sc = i.scale();
        i.set_ruler(true);
        i.measure_click(sc.x(NOW - 2.0 * H), 0.0);
        i.set_ruler(false);
        assert!(i.measure.is_none());
    }
}
