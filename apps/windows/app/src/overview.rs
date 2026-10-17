//! Overview pane: one line per selected account on shared axes. Reuses
//! `chart::Interaction` for range/zoom/hover/size and `chart_model` for
//! geometry; this module only adds the per-account series and the toggles.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use claude_dashboard_core::chart::{
    limit_points, rate_at, weighted_average_rate, with_reset_transitions, Entry,
};
use claude_dashboard_core::store::UsageLogStore;
use slint::{ComponentHandle, Model};

use crate::chart::{now_ms, offset_ms, to_state, vis_with_context, Interaction, Mode, View};
use crate::chart_model::{format_stamp, interpolate, points_to_entries, series_path, x_to_ms};
use crate::{AppWindow, ChartLegendItem, ChartLine, ChartPoint};

/// macOS `OverviewChartView.initialPreset` (.fiveHour).
const DEFAULT_PRESET: &str = "5h";
const DEFAULT_WINDOW: &str = "5h";
/// How far back the loaded history reaches (covers the 30d preset and pans).
const HISTORY_DAYS: f64 = 400.0;

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Window selector value -> store window code (5h=0, 7d=1, Fable=3).
pub fn window_code(w: &str) -> i64 {
    match w {
        "7d" => 1,
        "fable" => 3,
        _ => 0,
    }
}

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub color: slint::Color,
    /// Plan weight for the Avg overlay (macOS `planWeight`).
    pub weight: f64,
    /// Burn animal per window, in selector order: 5h, 7d, Fable ("" = unknown).
    pub animals: [String; 3],
}

/// macOS `OverviewChartView.planWeight`, from the plan label.
pub fn plan_weight(plan: &str) -> f64 {
    match plan {
        "Max 5x" => 5.0,
        "Max 20x" => 20.0,
        "Max" => 10.0,
        _ => 1.0,
    }
}

/// One frame of the Overview chart: the view state, the lines, the legend and
/// the limit-hit markers.
pub type OverviewView = (View, Vec<ChartLine>, Vec<ChartLegendItem>, Vec<(f64, f64)>);

pub struct Overview {
    inner: Interaction,
    window: String,
    accounts: Vec<Account>,
    /// Accounts with at least one point in the loaded window (absent == no line).
    series: BTreeMap<String, Vec<Entry>>,
    /// The limited samples per account (red dot + warning).
    limited: BTreeMap<String, Vec<Entry>>,
    hidden: HashSet<String>,
}

impl Overview {
    pub fn new(window: &str, preset: &str, accounts: Vec<Account>, now: f64) -> Self {
        // Same container as History: Pan by default, toggle to Zoom.
        let inner = Interaction::new(preset, now);
        Self {
            inner,
            window: window.to_string(),
            accounts,
            series: BTreeMap::new(),
            limited: BTreeMap::new(),
            hidden: HashSet::new(),
        }
    }

    pub fn loaded(
        &mut self,
        series: BTreeMap<String, Vec<Entry>>,
        limited: BTreeMap<String, Vec<Entry>>,
        now: f64,
    ) {
        self.series = series;
        self.limited = limited;
        self.inner.loaded(Vec::new(), now);
    }

    fn selected(&self) -> impl Iterator<Item = (&Account, &Vec<Entry>)> {
        self.accounts.iter().filter_map(move |a| {
            if self.hidden.contains(&a.id) {
                return None;
            }
            self.series
                .get(&a.id)
                .filter(|v| !v.is_empty())
                .map(|v| (a, v))
        })
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.hidden.remove(id) {
            self.hidden.insert(id.to_string());
        }
    }

    /// The animal for the active window selector (macOS `animalForSelectedWindow`).
    fn animal(&self, a: &Account) -> String {
        let i = match self.window.as_str() {
            "7d" => 1,
            "fable" => 2,
            _ => 0,
        };
        a.animals[i].clone()
    }

    pub fn view(&self, local_offset_ms: f64) -> OverviewView {
        let mut v = self.inner.view(local_offset_ms);
        let sc = self.inner.scale();
        let range = self.inner.range;
        let hover = self
            .inner
            .hover_pos()
            .filter(|&(hx, hy)| hx >= sc.left && hx <= sc.right && hy >= sc.top && hy <= sc.bottom);

        let mut lines = Vec::new();
        let mut limits = Vec::new();
        for (a, entries) in self.selected() {
            let path = series_path(&vis_with_context(entries, range), &sc);
            if path.is_empty() {
                continue; // nothing in the zoomed range: absent, not zero
            }
            if let Some(l) = self.limited.get(&a.id) {
                limits.extend(
                    l.iter()
                        .filter(|e| e.ms >= range.from_ms && e.ms <= range.to_ms)
                        .map(|e| (sc.x(e.ms), sc.y(e.value))),
                );
            }
            lines.push(ChartLine {
                path: path.as_str().into(),
                color: a.color,
                dot_visible: false,
                dot_x: 0.0,
                dot_y: 0.0,
            });
        }

        // As macOS: the line follows the pointer; the card lists every shown
        // account's interpolated value and local burn rate at that instant.
        let shown: Vec<&Account> = self
            .accounts
            .iter()
            .filter(|a| !self.hidden.contains(&a.id))
            .collect();
        match hover {
            Some((hx, hy)) if !shown.is_empty() => {
                let ms = x_to_ms(hx, &sc);
                let mut tip = vec![format_stamp(ms + local_offset_ms)];
                for a in &shown {
                    let es = self.series.get(&a.id).map(Vec::as_slice).unwrap_or(&[]);
                    let util = interpolate(es, ms)
                        .map(|u| format!("  {u:.0}%"))
                        .unwrap_or_default();
                    let rate = rate_at(es, ms)
                        .map(|r| format!("{r:+.1}%/h"))
                        .unwrap_or_else(|| "--".to_string());
                    tip.push(format!("{}{util}  {rate}", a.name));
                }
                v.cross = Some((hx, hy));
                v.tooltip = tip.join("\n");
            }
            _ => {
                v.cross = None;
                v.tooltip.clear();
            }
        }
        v.line_path.clear();
        v.empty = self.series.values().all(|s| s.is_empty());

        // Plan-weighted Avg over the visible range (macOS `weightedAverageRate`).
        let pairs: Vec<(&[Entry], f64)> = self
            .selected()
            .map(|(a, e)| (e.as_slice(), a.weight))
            .collect();
        (v.avg_text, v.avg_hot) = match weighted_average_rate(&pairs, range.from_ms, range.to_ms) {
            Some(r) => (format!("Avg: {r:.1}%/h"), r > 0.1),
            None => ("Avg: --".to_string(), false),
        };

        let legend = self
            .accounts
            .iter()
            .map(|a| ChartLegendItem {
                id: a.id.as_str().into(),
                name: a.name.as_str().into(),
                color: a.color,
                on: !self.hidden.contains(&a.id),
                has_data: self.series.get(&a.id).is_some_and(|s| !s.is_empty()),
                animal: self.animal(a).as_str().into(),
            })
            .collect();
        (v, lines, legend, limits)
    }
}

thread_local! {
    static STATE: RefCell<Option<Overview>> = const { RefCell::new(None) };
    /// The chart reports its size on init, possibly before `overview_open` builds the state.
    static LAST_SIZE: std::cell::Cell<Option<(f64, f64)>> = const { std::cell::Cell::new(None) };
}

fn push(app: &AppWindow, f: impl FnOnce(&mut Overview)) {
    let out = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let o = s.as_mut()?;
        f(o);
        let (v, lines, legend, limits) = o.view(offset_ms());
        Some((v, lines, legend, limits, o.window.clone()))
    });
    if let Some((v, lines, legend, limits, window)) = out {
        let mut st = to_state(&v);
        // No single series to put a dot on: the per-line dots carry the hover.
        st.cross_dot = false;
        st.limit_points = slint::ModelRc::new(slint::VecModel::from(
            limits
                .into_iter()
                .map(|(x, y)| ChartPoint {
                    x: x as f32,
                    y: y as f32,
                })
                .collect::<Vec<_>>(),
        ));
        st.lines = slint::ModelRc::new(slint::VecModel::from(lines));
        st.legend = slint::ModelRc::new(slint::VecModel::from(legend));
        st.window = window.as_str().into();
        app.set_overview(st);
    }
}

fn accounts_of(app: &AppWindow) -> Vec<Account> {
    let rows = app.get_account_rows();
    (0..rows.row_count())
        .filter_map(|i| rows.row_data(i))
        .map(|r| Account {
            id: r.id.to_string(),
            name: r.name.to_string(),
            color: r.avatar_color,
            weight: plan_weight(r.plan.as_str()),
            animals: [
                r.five_hour.animal.to_string(),
                r.seven_day.animal.to_string(),
                r.fable.animal.to_string(),
            ],
        })
        .collect()
}

type Series = BTreeMap<String, Vec<Entry>>;

/// The whole window's history per account (so pan / zoom stay in memory) and
/// each account's limited samples.
fn read_all(code: i64, now: f64) -> (Series, Series) {
    let store = match UsageLogStore::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("overview: usage log unavailable: {e}");
            return (Series::new(), Series::new());
        }
    };
    let from = (now - HISTORY_DAYS * 86_400_000.0) / 1000.0;
    let mut series = Series::new();
    let mut limited = Series::new();
    for (id, pts) in store.series_all(code, from, now / 1000.0) {
        let pts = with_reset_transitions(&pts);
        let entries = points_to_entries(&pts);
        if entries.is_empty() {
            continue;
        }
        let pairs: Vec<(Entry, bool)> = entries
            .iter()
            .copied()
            .zip(pts.iter().map(|p| p.is_limited))
            .collect();
        limited.insert(id.clone(), limit_points(&pairs));
        series.insert(id, entries);
    }
    (series, limited)
}

/// Read `series_all` for the current window on a worker; apply on the UI
/// thread unless superseded.
fn load(app: &AppWindow) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(window) = STATE.with(|s| s.borrow().as_ref().map(|o| o.window.clone())) else {
        return;
    };
    let code = window_code(&window);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let now = now_ms();
        let (series, limited) = read_all(code, now);
        let _ = slint::invoke_from_event_loop(move || {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            if let Some(app) = weak.upgrade() {
                push(&app, |o| o.loaded(series, limited, now));
            }
        });
    });
}

pub fn install(app: &AppWindow) {
    {
        let w = app.as_weak();
        app.on_overview_open(move || {
            let Some(app) = w.upgrade() else { return };
            let accounts = accounts_of(&app);
            // Keep toggles across re-opens.
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                let hidden = s.as_ref().map(|o| o.hidden.clone()).unwrap_or_default();
                let mut o = Overview::new(DEFAULT_WINDOW, DEFAULT_PRESET, accounts, now_ms());
                o.hidden = hidden;
                if let Some((cw, ch)) = LAST_SIZE.with(|l| l.get()) {
                    o.inner.set_size(cw, ch);
                }
                *s = Some(o);
            });
            push(&app, |_| {});
            load(&app);
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_close(move || {
            GENERATION.fetch_add(1, Ordering::SeqCst);
            if let Some(app) = w.upgrade() {
                app.set_overview(Default::default());
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_window(move |win| {
            let Some(app) = w.upgrade() else { return };
            let win = win.to_string();
            push(&app, |o| {
                o.window = win;
                o.series.clear();
                o.limited.clear();
                o.inner.clear_for_reload();
            });
            load(&app);
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_preset(move |p| {
            let Some(app) = w.upgrade() else { return };
            let p = p.to_string();
            // The whole history is loaded: a preset only moves the range.
            push(&app, |o| o.inner.set_preset(&p, now_ms()));
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_toggle(move |id| {
            if let Some(app) = w.upgrade() {
                push(&app, |o| o.toggle(&id));
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_size(move |cw, ch| {
            LAST_SIZE.with(|l| l.set(Some((cw as f64, ch as f64))));
            if let Some(app) = w.upgrade() {
                push(&app, |o| o.inner.set_size(cw as f64, ch as f64));
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_hover(move |x, y| {
            if let Some(app) = w.upgrade() {
                push(&app, |o| {
                    if x < 0.0 || y < 0.0 {
                        o.inner.hover_off();
                    } else {
                        o.inner.hover_at(x as f64, y as f64);
                    }
                });
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_select(move |a, b| {
            if let Some(app) = w.upgrade() {
                push(&app, |o| o.inner.select(a as f64, b as f64));
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_pan(move |dx| {
            if let Some(app) = w.upgrade() {
                push(&app, |o| o.inner.pan_by_px(dx as f64));
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_toggle_mode(move || {
            if let Some(app) = w.upgrade() {
                push(&app, |o| {
                    let next = if o.inner.mode == Mode::Pan {
                        Mode::Zoom
                    } else {
                        Mode::Pan
                    };
                    o.inner.set_mode(next);
                });
            }
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_reset(move || {
            if let Some(app) = w.upgrade() {
                push(&app, |o| o.inner.reset());
            }
        });
    }
}

/// A refresh finished: refresh the legend animals, slide a following range to
/// "now" and reload so fresh points show. No-op when the pane is not open.
pub fn on_refresh(app: &AppWindow) {
    if app.get_selection() != crate::Pane::Overview || STATE.with(|s| s.borrow().is_none()) {
        return;
    }
    let accounts = accounts_of(app);
    push(app, |o| {
        o.accounts = accounts;
        o.inner.live_tick(now_ms());
    });
    load(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f64 = 3_600_000.0;
    const NOW: f64 = 1_000.0 * 86_400_000.0;

    fn acct(id: &str) -> Account {
        Account {
            id: id.into(),
            name: id.to_uppercase(),
            color: slint::Color::from_rgb_u8(1, 2, 3),
            weight: 1.0,
            animals: [format!("{id}-5h"), format!("{id}-7d"), format!("{id}-f")],
        }
    }

    fn ov() -> Overview {
        let mut o = Overview::new("5h", "5h", vec![acct("a"), acct("b"), acct("c")], NOW);
        o.inner.set_size(536.0, 336.0);
        let mut m = BTreeMap::new();
        m.insert(
            "a".to_string(),
            vec![
                Entry {
                    ms: NOW - 3.0 * H,
                    value: 10.0,
                },
                Entry {
                    ms: NOW - 1.0 * H,
                    value: 30.0,
                },
            ],
        );
        m.insert(
            "b".to_string(),
            vec![Entry {
                ms: NOW - 2.0 * H,
                value: 50.0,
            }],
        );
        o.loaded(m, BTreeMap::new(), NOW);
        o
    }

    #[test]
    fn opens_on_the_5h_preset_like_macos() {
        assert_eq!(DEFAULT_PRESET, "5h");
        let o = Overview::new(DEFAULT_WINDOW, DEFAULT_PRESET, Vec::new(), NOW);
        assert_eq!(o.inner.preset, "5h");
    }

    #[test]
    fn window_codes() {
        assert_eq!(
            (window_code("5h"), window_code("7d"), window_code("fable")),
            (0, 1, 3)
        );
    }

    #[test]
    fn account_without_data_is_absent_not_zero() {
        let o = ov();
        let (v, lines, legend, _) = o.view(0.0);
        assert_eq!(lines.len(), 2, "c has no data -> no line");
        assert!(!v.empty);
        assert!(!legend[2].has_data && legend[0].has_data);
    }

    #[test]
    fn toggle_hides_and_shows_line() {
        let mut o = ov();
        o.toggle("a");
        assert_eq!(o.view(0.0).1.len(), 1);
        assert!(!o.view(0.0).2[0].on);
        o.toggle("a");
        assert_eq!(o.view(0.0).1.len(), 2);
    }

    #[test]
    fn hover_lists_each_selected_account() {
        let mut o = ov();
        let sc = o.inner.scale();
        o.inner.hover_at(sc.x(NOW - 2.0 * H), sc.top + 5.0);
        let (v, _lines, _, _) = o.view(0.0);
        assert!(v.cross.is_some());
        assert!(
            v.tooltip.contains("A  ") && v.tooltip.contains("B  50%"),
            "{}",
            v.tooltip
        );
    }

    #[test]
    fn hover_tooltip_interpolates_and_shows_rate_like_macos() {
        let mut o = ov();
        let sc = o.inner.scale();
        // a: 10% at -3h, 30% at -1h -> 20% at -2h, +10%/h. b has one sample: 50%, no rate.
        o.inner.hover_at(sc.x(NOW - 2.0 * H), sc.top + 5.0);
        let (v, lines, _, _) = o.view(0.0);
        assert!(v.tooltip.contains("A  20%  +10.0%/h"), "{}", v.tooltip);
        assert!(v.tooltip.contains("B  50%  --"), "{}", v.tooltip);
        assert!(
            v.tooltip.contains("C  --"),
            "no data: rate only, {}",
            v.tooltip
        );
        // The line follows the pointer exactly (no snapping to a sample).
        let hx = sc.x(NOW - 2.2 * H);
        o.inner.hover_at(hx, sc.top + 5.0);
        assert_eq!(o.view(0.0).0.cross.map(|c| c.0), Some(hx));
        assert!(o.view(0.0).1.iter().all(|l| !l.dot_visible));
        let _ = lines;
    }

    #[test]
    fn avg_rate_text_uses_weighted_rate() {
        // Task 3's literal: a (w1) climbs 10 -> 30 (+20) and 20 -> 50 (+30);
        // b (w5) +10; c (w20) has one point. Sum = (50*1 + 10*5 + 0*20) / 26 per 10h.
        let mut o = Overview::new("5h", "5h", vec![acct("a"), acct("b"), acct("c")], NOW);
        o.accounts[1].weight = 5.0;
        o.accounts[2].weight = 20.0;
        o.inner.range = claude_dashboard_core::chart::Range {
            from_ms: NOW - 10.0 * H,
            to_ms: NOW,
        };
        let e = |h: f64, v: f64| Entry {
            ms: NOW - 10.0 * H + h * H,
            value: v,
        };
        let mut m = BTreeMap::new();
        m.insert(
            "a".to_string(),
            vec![e(1.0, 10.0), e(2.0, 30.0), e(3.0, 20.0), e(4.0, 50.0)],
        );
        m.insert("b".to_string(), vec![e(1.0, 0.0), e(2.0, 10.0)]);
        m.insert("c".to_string(), vec![e(5.0, 70.0)]);
        o.inner.custom = true; // keep the range
        o.loaded(m, BTreeMap::new(), NOW);
        let (v, ..) = o.view(0.0);
        let want = format!("Avg: {:.1}%/h", 100.0 / 26.0 / 10.0);
        assert_eq!(v.avg_text, want);
        assert!(v.avg_hot, "0.38%/h is above 0.1");
        // toggling b off drops its weight from the mean
        o.toggle("b");
        o.toggle("c");
        assert_eq!(
            o.view(0.0).0.avg_text,
            format!("Avg: {:.1}%/h", 50.0 / 10.0)
        );
        o.toggle("a");
        assert_eq!(o.view(0.0).0.avg_text, "Avg: --");
    }

    #[test]
    fn legend_carries_animal() {
        let mut o = ov();
        assert_eq!(o.view(0.0).2[0].animal.as_str(), "a-5h");
        o.window = "7d".into();
        assert_eq!(o.view(0.0).2[1].animal.as_str(), "b-7d");
        o.window = "fable".into();
        assert_eq!(o.view(0.0).2[2].animal.as_str(), "c-f");
    }

    #[test]
    fn plan_weights_follow_macos() {
        assert_eq!(
            ["Pro", "Max 5x", "Max 20x", "Max"].map(plan_weight),
            [1.0, 5.0, 20.0, 10.0]
        );
    }

    #[test]
    fn noon_rules_and_limit_points_are_drawn() {
        let mut o = ov();
        // a 48h range always contains a local noon (offset 0 here).
        o.inner.set_preset("3d", NOW);
        let mut lim = BTreeMap::new();
        lim.insert(
            "a".to_string(),
            vec![Entry {
                ms: NOW - 1.0 * H,
                value: 100.0,
            }],
        );
        let series = o.series.clone();
        o.loaded(series, lim, NOW);
        let (v, _, _, limits) = o.view(0.0);
        assert!(!v.noon_path.is_empty());
        assert_eq!(limits.len(), 1);
        // hiding the account hides its limit marker
        o.toggle("a");
        assert!(o.view(0.0).3.is_empty());
    }

    #[test]
    fn overview_defaults_to_pan_and_toggles_to_zoom() {
        let mut o = ov();
        assert!(o.view(0.0).0.pan_mode);
        o.inner.set_mode(Mode::Zoom);
        assert!(!o.view(0.0).0.pan_mode);
    }

    #[test]
    fn pan_sets_custom_and_keeps_series() {
        let mut o = ov();
        o.inner.pan_by_px(40.0);
        let (v, lines, ..) = o.view(0.0);
        assert!(v.custom);
        assert!(lines.len() <= 2);
        assert!(!o.series.is_empty());
    }

    #[test]
    fn zoomed_out_of_range_account_disappears() {
        let mut o = ov();
        let sc = o.inner.scale();
        // zoom onto 1.5h..0.5h ago: a keeps its 1h-ago sample, b (2h ago) leaves.
        o.inner.select(sc.x(NOW - 1.5 * H), sc.x(NOW - 0.5 * H));
        assert_eq!(o.view(0.0).1.len(), 1);
    }
}
