//! The per-account History chart (macOS `AccountDetailView`): the reusable
//! `chart::Interaction` container plus what only this chart has - the whole
//! window's history loaded off the UI thread, limit points, the 5h reset
//! markers on the 7d window and the Reset Cycles list. The `.slint` side is
//! `HistoryBox` in `chart.slint`; its input arrives through the
//! `ChartActions` global and the result is published to `HistoryData`.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use claude_dashboard_core::chart::{limit_points, with_reset_transitions, Entry};
use claude_dashboard_core::colors::usage_color;
use claude_dashboard_core::store::{ResetCycle, UsageLogStore};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::chart::{
    frame_top, now_ms, offset_ms, to_state, Interaction, Mode, View, DEFAULT_PRESET,
};
use crate::chart_model::{
    dashed_hlines, dashed_vlines, day_bands, format_stamp, points_to_entries, rects_path,
};
use crate::model::to_color;
use crate::overview::window_code;
use crate::{AppWindow, ChartActions, ChartCycle, ChartPoint, ChartState, ChartTick, HistoryData};

/// How far back the loaded history reaches (covers the 30d preset and pans).
const HISTORY_DAYS: f64 = 400.0;

/// Bumped on every open / window change / close so a slow worker never
/// overwrites a newer view.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Everything one worker read for one account + window.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub entries: Vec<Entry>,
    pub limited: Vec<Entry>,
    pub cycles: Vec<ResetCycle>,
    /// 5h reset instants (ms); only read for the 7d window.
    pub five_h_resets: Vec<f64>,
}

/// The chart parts the shared `View` does not carry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extras {
    /// Every other local day shaded (macOS `chartDayBands`).
    pub band_path: String,
    /// The red dashed 100% limit rule.
    pub limit_path: String,
    pub reset_path: String,
    pub reset_xs: Vec<f64>,
    pub limit_points: Vec<(f64, f64)>,
    /// (range label, peak %, points, selected) newest first.
    pub cycles: Vec<(String, f64, usize, bool)>,
    pub has_cycle: bool,
}

pub struct History {
    id: String,
    pub inner: Interaction,
    limited: Vec<Entry>,
    cycles: Vec<ResetCycle>,
    five_h_resets: Vec<f64>,
}

impl History {
    pub fn new(id: &str, window: i64, now: f64) -> Self {
        let mut inner = Interaction::new(DEFAULT_PRESET, now);
        inner.set_window(window);
        inner.set_mode(Mode::Pan);
        Self {
            id: id.to_string(),
            inner,
            limited: Vec::new(),
            cycles: Vec::new(),
            five_h_resets: Vec::new(),
        }
    }

    pub fn loaded(&mut self, l: Loaded, now: f64) {
        self.limited = l.limited;
        self.cycles = l.cycles;
        self.five_h_resets = l.five_h_resets;
        self.inner.loaded(l.entries, now);
        // A selected cycle that no longer exists (the log was trimmed) is dropped.
        if let Some(c) = self.inner.selected_cycle {
            if !self
                .cycles
                .iter()
                .any(|x| x.resets_at_unix == c.resets_at_unix)
            {
                self.inner.select_cycle(None);
            }
        }
    }

    /// The window picker: new window, data reloaded by the caller.
    pub fn switch_window(&mut self, window: i64) {
        self.inner.set_window(window);
        self.inner.clear_for_reload();
        self.limited.clear();
        self.cycles.clear();
        self.five_h_resets.clear();
    }

    pub fn select_cycle_at(&mut self, index: usize) {
        if let Some(c) = self.cycles.get(index).copied() {
            self.inner.select_cycle(Some(c));
        }
    }

    pub fn view(&self, local_offset_ms: f64) -> (View, Extras) {
        let v = self.inner.view(local_offset_ms);
        let sc = self.inner.scale();
        let r = self.inner.range;
        let in_range = |ms: f64| ms >= r.from_ms && ms <= r.to_ms;

        let limit_points = self
            .limited
            .iter()
            .filter(|e| in_range(e.ms))
            .map(|e| (sc.x(e.ms), sc.y(e.value)))
            .collect();
        // macOS draws the 5h resets only while the 7d window is shown.
        let reset_xs: Vec<f64> = if self.inner.window == 1 {
            self.five_h_resets
                .iter()
                .copied()
                .filter(|&ms| in_range(ms))
                .map(|ms| sc.x(ms))
                .collect()
        } else {
            Vec::new()
        };
        let selected = self.inner.selected_cycle.map(|c| c.resets_at_unix);
        let cycles = self
            .cycles
            .iter()
            .map(|c| {
                (
                    format!(
                        "{} \u{2013} {}",
                        format_stamp(c.first_unix * 1000.0 + local_offset_ms),
                        format_stamp(c.resets_at_unix * 1000.0 + local_offset_ms)
                    ),
                    c.peak,
                    c.points,
                    selected == Some(c.resets_at_unix),
                )
            })
            .collect();
        // Bands over the data's days (not the range's), clipped to the range.
        let es = self.inner.entries();
        let bands: Vec<(f64, f64)> = match (es.first(), es.last()) {
            (Some(f), Some(l)) => day_bands(f.ms, l.ms, local_offset_ms)
                .into_iter()
                .filter(|&(a, b)| b > r.from_ms && a < r.to_ms)
                .map(|(a, b)| (sc.x(a.max(r.from_ms)), sc.x(b.min(r.to_ms))))
                .collect(),
            _ => Vec::new(),
        };
        let top = frame_top(&sc);
        let extras = Extras {
            band_path: rects_path(&bands, top, sc.bottom),
            limit_path: dashed_hlines(&[sc.y(100.0)], sc.left, sc.right, 5.0, 5.0),
            reset_path: dashed_vlines(&reset_xs, top, sc.bottom, 2.0, 4.0),
            reset_xs,
            limit_points,
            cycles,
            has_cycle: selected.is_some(),
        };
        (v, extras)
    }
}

thread_local! {
    static STATE: RefCell<Option<History>> = const { RefCell::new(None) };
    /// The chart's last reported size: it can arrive before the first `open`.
    static LAST_SIZE: std::cell::Cell<Option<(f64, f64)>> = const { std::cell::Cell::new(None) };
}

fn window_name(code: i64) -> &'static str {
    match code {
        1 => "7d",
        3 => "fable",
        _ => "5h",
    }
}

fn to_chart_state(v: &View, x: &Extras, window: i64) -> ChartState {
    let mut st = to_state(v);
    st.window = window_name(window).into();
    st.hint = "Data will appear after the next refresh.".into();
    st.band_path = x.band_path.as_str().into();
    st.limit_path = x.limit_path.as_str().into();
    st.reset_path = x.reset_path.as_str().into();
    st.reset_marks = ModelRc::new(VecModel::from(
        x.reset_xs
            .iter()
            .map(|&p| ChartTick {
                pos: p as f32,
                label: "\u{21ba}".into(),
            })
            .collect::<Vec<_>>(),
    ));
    st.limit_points = ModelRc::new(VecModel::from(
        x.limit_points
            .iter()
            .map(|&(px, py)| ChartPoint {
                x: px as f32,
                y: py as f32,
            })
            .collect::<Vec<_>>(),
    ));
    st.cycles = ModelRc::new(VecModel::from(
        x.cycles
            .iter()
            .map(|(label, peak, pts, selected)| ChartCycle {
                label: label.as_str().into(),
                peak: format!("peak: {}%", *peak as i64).into(),
                points: format!("{pts} pts").into(),
                color: to_color(usage_color(*peak)),
                selected: *selected,
            })
            .collect::<Vec<_>>(),
    ));
    st.has_cycle = x.has_cycle;
    st
}

/// Run `f` on the current history (if any) and publish the new state.
fn update(app: &AppWindow, f: impl FnOnce(&mut History)) {
    let out = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let h = s.as_mut()?;
        f(h);
        let (v, x) = h.view(offset_ms());
        Some(to_chart_state(&v, &x, h.inner.window))
    });
    if let Some(st) = out {
        app.global::<HistoryData>().set_state(st);
    }
}

fn read_loaded(account_id: &str, window: i64, now: f64) -> Loaded {
    let store = match UsageLogStore::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("history: usage log unavailable: {e}");
            return Loaded::default();
        }
    };
    let from = (now - HISTORY_DAYS * 86_400_000.0) / 1000.0;
    let points = with_reset_transitions(&store.series(account_id, window, from, now / 1000.0));
    let pairs: Vec<(Entry, bool)> = points
        .iter()
        .map(|p| {
            (
                Entry {
                    ms: p.t_unix * 1000.0,
                    value: p.utilization,
                },
                p.is_limited,
            )
        })
        .collect();
    let five_h_resets = if window == 1 {
        store
            .reset_cycles(account_id, 0)
            .iter()
            .map(|c| c.resets_at_unix * 1000.0)
            .collect()
    } else {
        Vec::new()
    };
    Loaded {
        entries: points_to_entries(&points),
        limited: limit_points(&pairs),
        cycles: store.reset_cycles(account_id, window),
        five_h_resets,
    }
}

/// Read the current account + window on a worker; apply on the UI thread
/// unless a newer request superseded it.
fn load(app: &AppWindow) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let Some((id, window)) =
        STATE.with(|s| s.borrow().as_ref().map(|h| (h.id.clone(), h.inner.window)))
    else {
        return;
    };
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let now = now_ms();
        let loaded = read_loaded(&id, window, now);
        let _ = slint::invoke_from_event_loop(move || {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            if let Some(app) = weak.upgrade() {
                update(&app, |h| h.loaded(loaded, now));
            }
        });
    });
}

/// A refresh finished: slide the live window to "now" and reload so fresh
/// points show (macOS `handleLiveTick`). No-op when no chart is open.
pub fn on_refresh(app: &AppWindow) {
    // Nothing open, or the ruler is armed: macOS `handleLiveTick` returns early
    // (the measure anchors on absolute dates), so neither re-anchor nor reload.
    if STATE.with(|s| s.borrow().as_ref().is_none_or(|h| h.inner.ruler)) {
        return;
    }
    update(app, |h| {
        h.inner.live_tick(now_ms());
    });
    load(app);
}

/// Wire the chart actions of the History box.
pub fn install(app: &AppWindow) {
    let a = app.global::<ChartActions>();
    {
        let w = app.as_weak();
        a.on_open(move |id, window| {
            let Some(app) = w.upgrade() else { return };
            if id.is_empty() {
                GENERATION.fetch_add(1, Ordering::SeqCst);
                STATE.with(|s| *s.borrow_mut() = None);
                app.global::<HistoryData>().set_state(ChartState::default());
                return;
            }
            let code = window_code(window.as_str());
            let same = STATE.with(|s| s.borrow().as_ref().is_some_and(|h| h.id == id.as_str()));
            if same {
                update(&app, |h| h.switch_window(code));
            } else {
                let mut h = History::new(id.as_str(), code, now_ms());
                if let Some((cw, ch)) = LAST_SIZE.with(|l| l.get()) {
                    h.inner.set_size(cw, ch);
                }
                STATE.with(|s| *s.borrow_mut() = Some(h));
                update(&app, |_| {});
            }
            load(&app);
        });
    }
    {
        let w = app.as_weak();
        a.on_preset(move |p| {
            if let Some(app) = w.upgrade() {
                update(&app, |h| h.inner.set_preset(p.as_str(), now_ms()));
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_toggle_mode(move || {
            if let Some(app) = w.upgrade() {
                update(&app, |h| {
                    let next = if h.inner.mode == Mode::Pan {
                        Mode::Zoom
                    } else {
                        Mode::Pan
                    };
                    h.inner.set_mode(next);
                });
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_toggle_ruler(move || {
            if let Some(app) = w.upgrade() {
                update(&app, |h| {
                    let on = !h.inner.ruler;
                    h.inner.set_ruler(on);
                });
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_size(move |cw, ch| {
            LAST_SIZE.with(|l| l.set(Some((cw as f64, ch as f64))));
            if let Some(app) = w.upgrade() {
                update(&app, |h| h.inner.set_size(cw as f64, ch as f64));
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_hover(move |x, y| {
            if let Some(app) = w.upgrade() {
                update(&app, |h| {
                    if x < 0.0 || y < 0.0 {
                        h.inner.hover_off();
                    } else {
                        h.inner.hover_at(x as f64, y as f64);
                    }
                });
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_select(move |x0, x1| {
            if let Some(app) = w.upgrade() {
                update(&app, |h| h.inner.zoom_select(x0 as f64, x1 as f64));
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_pan(move |dx| {
            if let Some(app) = w.upgrade() {
                update(&app, |h| h.inner.pan_by_px(dx as f64));
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_click(move |x, y| {
            if let Some(app) = w.upgrade() {
                update(&app, |h| {
                    if h.inner.ruler {
                        h.inner.measure_click(x as f64, y as f64);
                    }
                });
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_reset(move || {
            if let Some(app) = w.upgrade() {
                update(&app, |h| h.inner.reset());
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_select_cycle(move |i| {
            if let Some(app) = w.upgrade() {
                update(&app, |h| h.select_cycle_at(i.max(0) as usize));
            }
        });
    }
    {
        let w = app.as_weak();
        a.on_show_all(move || {
            if let Some(app) = w.upgrade() {
                update(&app, |h| h.inner.select_cycle(None));
            }
        });
    }
}

/// Dev seed (only under `CLAUDE_DASHBOARD_FAKE_ROWS`, never in production):
/// logs history for the fake accounts f2/f3 (f1 stays empty so the "No data
/// yet" state is reachable): a reset and a limited point inside the last 24h
/// on the 5h window, and 5 days of 7d points. Runs on a worker; skips an
/// account/window that already has a reset.
pub fn seed_fake_history() {
    std::thread::spawn(|| {
        let Ok(mut store) = UsageLogStore::open() else {
            return;
        };
        let now = now_ms() / 1000.0;
        // 5h window, one point every 40 min for ~26h: climbs to a limit, resets, climbs again.
        let five_h: [(&str, [f64; 40]); 2] = [
            (
                "f2",
                seed_5h(&[
                    (0, 8.0),
                    (5, 35.0),
                    (11, 78.0),
                    (13, 100.0),
                    (14, 0.0),
                    (24, 30.0),
                    (39, 64.0),
                ]),
            ),
            (
                "f3",
                seed_5h(&[
                    (0, 12.0),
                    (7, 60.0),
                    (14, 100.0),
                    (16, 100.0),
                    (17, 0.0),
                    (28, 55.0),
                    (39, 96.0),
                ]),
            ),
        ];
        for (id, vals) in five_h {
            if store.reset_cycles(id, 0).len() >= 2 {
                continue;
            }
            let n = vals.len() as f64;
            let reset_k = vals.iter().position(|&x| x == 0.0).unwrap_or(0);
            let reset_t = now - (n - 1.0 - reset_k as f64) * 2400.0;
            for (k, v) in vals.iter().enumerate() {
                let t = now - (n - 1.0 - k as f64) * 2400.0;
                let rat = if k < reset_k { reset_t } else { now + 7200.0 };
                store.record_at(id, 0, rat, *v, *v >= 100.0, t);
            }
        }
        // 7d window, one point every 6h for 5 days: three small steps and a reset.
        for (id, scale) in [("f2", 1.0_f64), ("f3", 1.4)] {
            if store.reset_cycles(id, 1).len() >= 2 {
                continue;
            }
            let vals: [f64; 20] = [
                5.0, 12.0, 20.0, 31.0, 44.0, 52.0, 60.0, 71.0, 83.0, 91.0, 0.0, 9.0, 18.0, 24.0,
                30.0, 38.0, 45.0, 52.0, 58.0, 66.0,
            ];
            let n = vals.len() as f64;
            let reset_k = vals.iter().position(|&x| x == 0.0).unwrap_or(0);
            let reset_t = now - (n - 1.0 - reset_k as f64) * 21_600.0;
            for (k, v) in vals.iter().enumerate() {
                let t = now - (n - 1.0 - k as f64) * 21_600.0;
                let rat = if k < reset_k { reset_t } else { now + 86_400.0 };
                store.record_at(id, 1, rat, (v * scale).min(100.0), false, t);
            }
        }
    });
}

/// Piecewise-linear fill of 40 samples from `(index, value)` knots; a drop to
/// a small value after 100 is the reset.
fn seed_5h(knots: &[(usize, f64)]) -> [f64; 40] {
    let mut out = [0.0; 40];
    for w in knots.windows(2) {
        let ((i0, v0), (i1, v1)) = (w[0], w[1]);
        for (i, slot) in out.iter_mut().enumerate().take(i1 + 1).skip(i0) {
            // The step right after a limit hit is the reset itself (no ramp).
            *slot = if i1 == i0 + 1 {
                if i == i0 {
                    v0
                } else {
                    v1
                }
            } else {
                v0 + (v1 - v0) * (i - i0) as f64 / (i1 - i0) as f64
            };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f64 = 3_600_000.0;
    const NOW: f64 = 1_000.0 * 86_400_000.0;

    fn e(h_ago: f64, v: f64) -> Entry {
        Entry {
            ms: NOW - h_ago * H,
            value: v,
        }
    }

    fn hist(window: i64) -> History {
        let mut h = History::new("a", window, NOW);
        h.inner.set_size(536.0, 336.0);
        h.inner.set_preset("7d", NOW);
        h.loaded(
            Loaded {
                entries: vec![e(30.0, 10.0), e(20.0, 100.0), e(10.0, 5.0), e(1.0, 20.0)],
                limited: vec![e(20.0, 100.0), e(300.0, 100.0)],
                cycles: vec![
                    ResetCycle {
                        resets_at_unix: (NOW - 10.0 * H) / 1000.0,
                        first_unix: (NOW - 20.0 * H) / 1000.0,
                        last_unix: (NOW - 1.0 * H) / 1000.0,
                        peak: 100.0,
                        points: 2,
                    },
                    ResetCycle {
                        resets_at_unix: (NOW - 30.0 * H) / 1000.0,
                        first_unix: (NOW - 40.0 * H) / 1000.0,
                        last_unix: (NOW - 31.0 * H) / 1000.0,
                        peak: 40.0,
                        points: 4,
                    },
                ],
                five_h_resets: vec![NOW - 12.0 * H, NOW - 400.0 * H],
            },
            NOW,
        );
        h
    }

    #[test]
    fn limit_points_only_inside_the_range() {
        let h = hist(0);
        let (_, x) = h.view(0.0);
        assert_eq!(x.limit_points.len(), 1, "the 300h-old point is outside 7d");
    }

    #[test]
    fn day_bands_and_limit_rule() {
        // Samples from 30h to 1h ago (NOW is a UTC midnight): days NOW-2d,
        // NOW-1d; only the second (odd) one is shaded.
        let h = hist(0);
        let (_, x) = h.view(0.0);
        assert_eq!(x.band_path.matches('Z').count(), 1, "{}", x.band_path);
        let sc = h.inner.scale();
        assert!(x
            .band_path
            .starts_with(&format!("M {:.2} ", sc.x(NOW - 24.0 * H))));
        assert!(x
            .limit_path
            .starts_with(&format!("M {:.2} {:.2} L", sc.left, sc.y(100.0))));
        // A 5h range inside the unshaded day has no band.
        let mut h = h;
        h.inner.set_preset("5h", NOW - 26.0 * H);
        assert!(h.view(0.0).1.band_path.is_empty());
    }

    #[test]
    fn five_hour_resets_only_on_the_seven_day_window() {
        let (_, on_7d) = hist(1).view(0.0);
        assert_eq!(on_7d.reset_xs.len(), 1);
        assert!(!on_7d.reset_path.is_empty());
        let (_, on_5h) = hist(0).view(0.0);
        assert!(on_5h.reset_xs.is_empty() && on_5h.reset_path.is_empty());
    }

    #[test]
    fn cycle_selection_marks_the_row_and_shows_all_clears_it() {
        let mut h = hist(0);
        h.select_cycle_at(1);
        let (v, x) = h.view(0.0);
        assert!(v.custom && x.has_cycle);
        assert_eq!(x.cycles.iter().filter(|c| c.3).count(), 1);
        assert!(x.cycles[1].3 && !x.cycles[0].3);
        h.inner.select_cycle(None);
        let (v, x) = h.view(0.0);
        assert!(!v.custom && !x.has_cycle);
    }

    #[test]
    fn switching_window_empties_until_the_reload_lands() {
        let mut h = hist(0);
        h.switch_window(1);
        let (v, x) = h.view(0.0);
        assert!(v.empty && v.loading && x.cycles.is_empty() && x.limit_points.is_empty());
        assert_eq!(h.inner.window, 1);
    }

    #[test]
    fn a_vanished_cycle_is_dropped_on_reload() {
        let mut h = hist(0);
        h.select_cycle_at(0);
        h.loaded(Loaded::default(), NOW);
        assert!(h.inner.selected_cycle.is_none());
    }

    #[test]
    fn seed_ramps_and_resets() {
        let v = seed_5h(&[(0, 8.0), (5, 35.0), (13, 100.0), (14, 0.0), (39, 64.0)]);
        assert_eq!(v[0], 8.0);
        assert_eq!(v[13], 100.0);
        assert_eq!(v[14], 0.0);
        assert_eq!(v[39], 64.0);
    }
}
