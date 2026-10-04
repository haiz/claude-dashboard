//! Per-account interactive usage chart: loads the logged series off the UI
//! thread and holds the interaction state (preset, range, hover). All
//! geometry comes from `core::chart` via `chart_model`; the `.slint` only
//! draws what this module computes.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use claude_dashboard_core::chart::{
    format_tick, make_scale, nearest_entry, zoom_range, Entry, Range, Scale,
};
use claude_dashboard_core::store::UsageLogStore;
use slint::ComponentHandle;

use crate::chart_model::{
    axis_ticks, points_to_entries, preset_window_and_span, series_path, x_to_ms,
};
use crate::{AppWindow, ChartState, ChartTick};

/// Uniform padding (px) around the plot; holds the axis labels.
const PAD: f64 = 36.0;
const DEFAULT_PRESET: &str = "24h";

/// Bumped on every open/preset/close so a slow worker never overwrites a newer view.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// What the UI should show; plain data so it is testable without Slint.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    pub line_path: String,
    pub x_ticks: Vec<(f64, String)>,
    pub y_ticks: Vec<(f64, String)>,
    pub cross: Option<(f64, f64)>,
    pub tooltip: String,
    pub empty: bool,
    pub loading: bool,
    pub preset: String,
    pub plot: (f64, f64, f64, f64),
}

#[derive(Debug, Clone)]
pub struct Interaction {
    pub preset: String,
    entries: Vec<Entry>,
    /// The preset's full range (what double-click resets to).
    base: Range,
    pub range: Range,
    size: (f64, f64),
    hover: Option<(f64, f64)>,
    loading: bool,
}

impl Interaction {
    pub fn new(preset: &str, now_ms: f64) -> Self {
        let base = preset_range(preset, now_ms);
        Self {
            preset: preset.to_string(),
            entries: Vec::new(),
            base,
            range: base,
            size: (640.0, 320.0),
            hover: None,
            loading: true,
        }
    }

    fn scale(&self) -> Scale {
        make_scale(self.range, self.size.0, self.size.1, PAD)
    }

    fn visible(&self) -> Vec<Entry> {
        self.entries
            .iter()
            .copied()
            .filter(|e| e.ms >= self.range.from_ms && e.ms <= self.range.to_ms)
            .collect()
    }

    pub fn set_size(&mut self, w: f64, h: f64) {
        if w > 0.0 && h > 0.0 {
            self.size = (w, h);
        }
    }

    pub fn start_loading(&mut self, preset: &str, now_ms: f64) {
        self.preset = preset.to_string();
        self.base = preset_range(preset, now_ms);
        self.range = self.base;
        self.entries.clear();
        self.hover = None;
        self.loading = true;
    }

    pub fn loaded(&mut self, entries: Vec<Entry>, now_ms: f64) {
        self.base = preset_range(&self.preset, now_ms);
        self.range = self.base;
        self.entries = entries;
        self.hover = None;
        self.loading = false;
    }

    pub fn hover_at(&mut self, x: f64, y: f64) {
        self.hover = Some((x, y));
    }

    pub fn hover_off(&mut self) {
        self.hover = None;
    }

    /// Drag-select between two pointer x positions (px): becomes the range,
    /// clamped to the visible range and to `zoom_range`'s span bounds.
    pub fn select(&mut self, x0: f64, x1: f64) {
        let sc = self.scale();
        let (a, b) = (x_to_ms(x0.min(x1), &sc), x_to_ms(x0.max(x1), &sc));
        let a = a.max(self.range.from_ms);
        let b = b.min(self.range.to_ms);
        if b <= a {
            return;
        }
        self.range = zoom_range(Range { from_ms: a, to_ms: b }, 1.0, (a + b) / 2.0);
        self.hover = None;
    }

    pub fn reset(&mut self) {
        self.range = self.base;
        self.hover = None;
    }

    pub fn view(&self, local_offset_ms: f64) -> View {
        let sc = self.scale();
        let span = self.range.to_ms - self.range.from_ms;
        let max_ticks = ((sc.plot_width / 90.0) as usize).max(2);
        let (xs, ys) = axis_ticks(self.range, &sc, max_ticks, local_offset_ms);
        let vis = self.visible();
        let empty = self.entries.is_empty();
        let mut cross = None;
        let mut tooltip = String::new();
        if let Some((hx, hy)) = self.hover {
            let inside = hx >= sc.left && hx <= sc.right && hy >= sc.top && hy <= sc.bottom;
            if inside {
                if let Some(i) = nearest_entry(&vis, x_to_ms(hx, &sc)) {
                    let e = vis[i];
                    cross = Some((sc.x(e.ms), sc.y(e.value)));
                    tooltip = format!(
                        "{}  {:.0}%",
                        format_tick(e.ms + local_offset_ms, span),
                        e.value
                    );
                }
            }
        }
        View {
            line_path: series_path(&vis_with_context(&self.entries, self.range), &sc),
            x_ticks: xs,
            y_ticks: ys.into_iter().map(|(v, py)| (py, format!("{v:.0}%"))).collect(),
            cross,
            tooltip,
            empty,
            loading: self.loading,
            preset: self.preset.clone(),
            plot: (sc.left, sc.top, sc.plot_width, sc.plot_height),
        }
    }
}

/// Entries inside the range plus one neighbour on each side, so the line
/// enters and leaves the plot edge instead of starting mid-air when zoomed
/// (the .slint clips the path to the plot frame).
fn vis_with_context(entries: &[Entry], r: Range) -> Vec<Entry> {
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

fn preset_range(preset: &str, now_ms: f64) -> Range {
    let (_, span) = preset_window_and_span(preset);
    Range { from_ms: now_ms - span, to_ms: now_ms }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn offset_ms() -> f64 {
    crate::model::local_offset_s() * 1000.0
}

thread_local! {
    static STATE: RefCell<Option<(String, Interaction)>> = const { RefCell::new(None) };
}

fn to_state(v: &View) -> ChartState {
    let ticks = |t: &[(f64, String)]| -> slint::ModelRc<ChartTick> {
        let items: Vec<ChartTick> = t
            .iter()
            .map(|(p, l)| ChartTick { pos: *p as f32, label: l.as_str().into() })
            .collect();
        slint::ModelRc::new(slint::VecModel::from(items))
    };
    let (cx, cy) = v.cross.unwrap_or((0.0, 0.0));
    ChartState {
        line_path: v.line_path.as_str().into(),
        x_ticks: ticks(&v.x_ticks),
        y_ticks: ticks(&v.y_ticks),
        cross_visible: v.cross.is_some(),
        cross_x: cx as f32,
        cross_y: cy as f32,
        tooltip: v.tooltip.as_str().into(),
        empty: v.empty,
        loading: v.loading,
        preset: v.preset.as_str().into(),
        plot_left: v.plot.0 as f32,
        plot_top: v.plot.1 as f32,
        plot_width: v.plot.2 as f32,
        plot_height: v.plot.3 as f32,
    }
}

/// Run `f` on the current interaction (if any) and push the new view.
fn update(app: &AppWindow, f: impl FnOnce(&mut Interaction)) {
    let view = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let (_, i) = s.as_mut()?;
        f(i);
        Some(i.view(offset_ms()))
    });
    if let Some(v) = view {
        app.set_chart(to_state(&v));
    }
}

/// Read the preset's series on a worker (the on-disk log) and apply it on the
/// UI thread unless a newer request superseded it.
fn load(app: &AppWindow, account_id: String, preset: String) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let (window, span) = preset_window_and_span(&preset);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let now = now_ms();
        let entries = match UsageLogStore::open() {
            Ok(store) => points_to_entries(&store.series(
                &account_id,
                window,
                (now - span) / 1000.0,
                now / 1000.0,
            )),
            Err(e) => {
                eprintln!("chart: usage log unavailable: {e}");
                Vec::new()
            }
        };
        let _ = slint::invoke_from_event_loop(move || {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            if let Some(app) = weak.upgrade() {
                update(&app, |i| i.loaded(entries, now));
            }
        });
    });
}

/// Wire the chart callbacks on the main window.
pub fn install(app: &AppWindow) {
    {
        let w = app.as_weak();
        app.on_view_chart(move |id| {
            let Some(app) = w.upgrade() else { return };
            let id = id.to_string();
            STATE.with(|s| {
                *s.borrow_mut() = Some((id.clone(), Interaction::new(DEFAULT_PRESET, now_ms())));
            });
            update(&app, |_| {});
            load(&app, id, DEFAULT_PRESET.to_string());
        });
    }
    {
        let w = app.as_weak();
        app.on_chart_preset(move |p| {
            let Some(app) = w.upgrade() else { return };
            let p = p.to_string();
            let Some(id) = STATE.with(|s| s.borrow().as_ref().map(|(id, _)| id.clone())) else {
                return;
            };
            update(&app, |i| i.start_loading(&p, now_ms()));
            load(&app, id, p);
        });
    }
    {
        let w = app.as_weak();
        app.on_chart_close(move || {
            GENERATION.fetch_add(1, Ordering::SeqCst);
            STATE.with(|s| *s.borrow_mut() = None);
            if let Some(app) = w.upgrade() {
                app.set_chart(ChartState::default());
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_chart_size(move |cw, ch| {
            if let Some(app) = w.upgrade() {
                update(&app, |i| i.set_size(cw as f64, ch as f64));
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_chart_hover(move |x, y| {
            if let Some(app) = w.upgrade() {
                update(&app, |i| {
                    if x < 0.0 || y < 0.0 {
                        i.hover_off();
                    } else {
                        i.hover_at(x as f64, y as f64);
                    }
                });
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_chart_select(move |a, b| {
            if let Some(app) = w.upgrade() {
                update(&app, |i| i.select(a as f64, b as f64));
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_chart_reset(move || {
            if let Some(app) = w.upgrade() {
                update(&app, |i| i.reset());
            }
        });
    }
}

/// Dev seed (only under `CLAUDE_DASHBOARD_FAKE_ROWS`, never in production):
/// logs a few points for the fake accounts f2/f3 (f1 stays empty so the
/// "No data yet" state is reachable). Runs on a worker; skips accounts that
/// already have points.
pub fn seed_fake_history() {
    std::thread::spawn(|| {
        let Ok(mut store) = UsageLogStore::open() else { return };
        let now = now_ms() / 1000.0;
        let shapes: [(&str, &[f64]); 2] = [
            ("f2", &[4.0, 9.0, 15.0, 22.0, 30.0, 41.0, 48.0, 55.0, 60.0, 64.0]),
            ("f3", &[10.0, 25.0, 38.0, 52.0, 66.0, 75.0, 83.0, 91.0, 96.0, 100.0]),
        ];
        for (id, vals) in shapes {
            for (window, step_s) in [(0_i64, 1500.0_f64), (1, 20000.0)] {
                if store.count(id, window) > 0 {
                    continue;
                }
                let n = vals.len() as f64;
                for (k, v) in vals.iter().enumerate() {
                    let t = now - (n - 1.0 - k as f64) * step_s;
                    store.record_at(id, window, now + 7200.0, *v, false, t);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f64 = 3_600_000.0;
    const NOW: f64 = 1_000.0 * 86_400_000.0;

    fn loaded() -> Interaction {
        let mut i = Interaction::new("5h", NOW);
        i.set_size(536.0, 336.0); // plot 464 x 264 with PAD 36
        i.loaded(
            vec![
                Entry { ms: NOW - 4.0 * H, value: 10.0 },
                Entry { ms: NOW - 2.0 * H, value: 40.0 },
                Entry { ms: NOW - 1.0 * H, value: 60.0 },
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
        assert!(v.empty && v.cross.is_none() && v.line_path.is_empty());
    }

    #[test]
    fn hover_inside_snaps_to_nearest_sample() {
        let mut i = loaded();
        let sc = i.scale();
        i.hover_at(sc.x(NOW - 2.1 * H), sc.top + 10.0);
        let v = i.view(0.0);
        let (cx, _) = v.cross.expect("crosshair");
        assert!((cx - sc.x(NOW - 2.0 * H)).abs() < 1e-6);
        assert!(v.tooltip.ends_with("40%"), "{}", v.tooltip);
    }

    #[test]
    fn hover_off_plot_hides_crosshair() {
        let mut i = loaded();
        i.hover_at(5.0, 5.0); // inside padding, outside plot
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

    #[test]
    fn start_loading_resets_state() {
        let mut i = loaded();
        i.start_loading("7d", NOW);
        let v = i.view(0.0);
        assert!(v.loading && v.preset == "7d");
        assert_eq!(i.range.to_ms - i.range.from_ms, 7.0 * 24.0 * H);
    }
}
