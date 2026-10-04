//! Overview pane: one line per selected account on shared axes. Reuses
//! `chart::Interaction` for range/zoom/hover/size and `chart_model` for
//! geometry; this module only adds the per-account series and the toggles.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use claude_dashboard_core::chart::{format_tick, nearest_entry, Entry};
use claude_dashboard_core::store::UsageLogStore;
use slint::{ComponentHandle, Model};

use crate::chart::{now_ms, offset_ms, to_state, vis_with_context, Interaction, View};
use crate::chart_model::{points_to_entries, preset_window_and_span, series_path, x_to_ms};
use crate::{AppWindow, ChartLegendItem, ChartLine};

const DEFAULT_PRESET: &str = "24h";
const DEFAULT_WINDOW: &str = "5h";

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
}

pub struct Overview {
    inner: Interaction,
    window: String,
    accounts: Vec<Account>,
    /// Accounts with at least one point in the loaded window (absent == no line).
    series: BTreeMap<String, Vec<Entry>>,
    hidden: HashSet<String>,
}

struct Hit {
    ms: f64,
    value: f64,
    name: String,
}

impl Overview {
    pub fn new(window: &str, preset: &str, accounts: Vec<Account>, now: f64) -> Self {
        Self {
            inner: Interaction::new(preset, now),
            window: window.to_string(),
            accounts,
            series: BTreeMap::new(),
            hidden: HashSet::new(),
        }
    }

    pub fn loaded(&mut self, series: BTreeMap<String, Vec<Entry>>, now: f64) {
        self.series = series;
        self.inner.loaded(Vec::new(), now);
    }

    fn selected(&self) -> impl Iterator<Item = (&Account, &Vec<Entry>)> {
        self.accounts.iter().filter_map(move |a| {
            if self.hidden.contains(&a.id) {
                return None;
            }
            self.series.get(&a.id).filter(|v| !v.is_empty()).map(|v| (a, v))
        })
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.hidden.remove(id) {
            self.hidden.insert(id.to_string());
        }
    }

    pub fn view(&self, local_offset_ms: f64) -> (View, Vec<ChartLine>, Vec<ChartLegendItem>) {
        let mut v = self.inner.view(local_offset_ms);
        let sc = self.inner.scale();
        let range = self.inner.range;
        let span = range.to_ms - range.from_ms;
        let hover = self.inner.hover_pos().filter(|&(hx, hy)| {
            hx >= sc.left && hx <= sc.right && hy >= sc.top && hy <= sc.bottom
        });
        let hover_ms = hover.map(|(hx, _)| x_to_ms(hx, &sc));

        let mut lines = Vec::new();
        let mut hits: Vec<(usize, Hit)> = Vec::new();
        for (a, entries) in self.selected() {
            let path = series_path(&vis_with_context(entries, range), &sc);
            if path.is_empty() {
                continue; // nothing in the zoomed range: absent, not zero
            }
            let vis: Vec<Entry> = entries
                .iter()
                .copied()
                .filter(|e| e.ms >= range.from_ms && e.ms <= range.to_ms)
                .collect();
            if let Some(ms) = hover_ms {
                if let Some(i) = nearest_entry(&vis, ms) {
                    hits.push((
                        lines.len(),
                        Hit { ms: vis[i].ms, value: vis[i].value, name: a.name.clone() },
                    ));
                }
            }
            lines.push(ChartLine {
                path: path.as_str().into(),
                color: a.color,
                dot_visible: false,
                dot_x: 0.0,
                dot_y: 0.0,
            });
        }

        // The crosshair snaps to the sample closest to the pointer across all
        // lines; the tooltip lists each account's nearest value.
        match hover_ms {
            Some(ms) if !hits.is_empty() => {
                let best = hits
                    .iter()
                    .map(|(_, h)| h.ms)
                    .min_by(|a, b| (a - ms).abs().total_cmp(&(b - ms).abs()))
                    .unwrap_or(ms);
                let mut tip = vec![format_tick(best + local_offset_ms, span)];
                for (i, h) in &hits {
                    lines[*i].dot_visible = true;
                    lines[*i].dot_x = sc.x(h.ms) as f32;
                    lines[*i].dot_y = sc.y(h.value) as f32;
                    tip.push(format!("{}  {:.0}%", h.name, h.value));
                }
                v.cross = Some((sc.x(best), hover.map(|(_, y)| y).unwrap_or(sc.top)));
                v.tooltip = tip.join("\n");
            }
            _ => {
                v.cross = None;
                v.tooltip.clear();
            }
        }
        v.line_path.clear();
        v.empty = self.series.values().all(|s| s.is_empty());

        let legend = self
            .accounts
            .iter()
            .map(|a| ChartLegendItem {
                id: a.id.as_str().into(),
                name: a.name.as_str().into(),
                color: a.color,
                on: !self.hidden.contains(&a.id),
                has_data: self.series.get(&a.id).is_some_and(|s| !s.is_empty()),
            })
            .collect();
        (v, lines, legend)
    }
}

thread_local! {
    static STATE: RefCell<Option<Overview>> = const { RefCell::new(None) };
}

fn push(app: &AppWindow, f: impl FnOnce(&mut Overview)) {
    let out = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let o = s.as_mut()?;
        f(o);
        let (v, lines, legend) = o.view(offset_ms());
        Some((v, lines, legend, o.window.clone()))
    });
    if let Some((v, lines, legend, window)) = out {
        let mut st = to_state(&v);
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
        .map(|r| Account { id: r.id.to_string(), name: r.name.to_string(), color: r.avatar_color })
        .collect()
}

/// Read `series_all` for the current window/preset on a worker; apply on the
/// UI thread unless superseded.
fn load(app: &AppWindow) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let Some((window, preset)) =
        STATE.with(|s| s.borrow().as_ref().map(|o| (o.window.clone(), o.inner.preset.clone())))
    else {
        return;
    };
    let code = window_code(&window);
    let (_, span) = preset_window_and_span(&preset);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let now = now_ms();
        let series: BTreeMap<String, Vec<Entry>> = match UsageLogStore::open() {
            Ok(store) => store
                .series_all(code, (now - span) / 1000.0, now / 1000.0)
                .into_iter()
                .map(|(id, pts)| (id, points_to_entries(&pts)))
                .filter(|(_, e)| !e.is_empty())
                .collect(),
            Err(e) => {
                eprintln!("overview: usage log unavailable: {e}");
                BTreeMap::new()
            }
        };
        let _ = slint::invoke_from_event_loop(move || {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            if let Some(app) = weak.upgrade() {
                push(&app, |o| o.loaded(series, now));
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
                let preset = o.inner.preset.clone();
                o.inner.start_loading(&preset, now_ms());
            });
            load(&app);
        });
    }
    {
        let w = app.as_weak();
        app.on_overview_preset(move |p| {
            let Some(app) = w.upgrade() else { return };
            let p = p.to_string();
            push(&app, |o| {
                o.series.clear();
                o.inner.start_loading(&p, now_ms());
            });
            load(&app);
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
        app.on_overview_reset(move || {
            if let Some(app) = w.upgrade() {
                push(&app, |o| o.inner.reset());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f64 = 3_600_000.0;
    const NOW: f64 = 1_000.0 * 86_400_000.0;

    fn acct(id: &str) -> Account {
        Account { id: id.into(), name: id.to_uppercase(), color: slint::Color::from_rgb_u8(1, 2, 3) }
    }

    fn ov() -> Overview {
        let mut o = Overview::new("5h", "5h", vec![acct("a"), acct("b"), acct("c")], NOW);
        o.inner.set_size(536.0, 336.0);
        let mut m = BTreeMap::new();
        m.insert(
            "a".to_string(),
            vec![Entry { ms: NOW - 3.0 * H, value: 10.0 }, Entry { ms: NOW - 1.0 * H, value: 30.0 }],
        );
        m.insert("b".to_string(), vec![Entry { ms: NOW - 2.0 * H, value: 50.0 }]);
        o.loaded(m, NOW);
        o
    }

    #[test]
    fn window_codes() {
        assert_eq!((window_code("5h"), window_code("7d"), window_code("fable")), (0, 1, 3));
    }

    #[test]
    fn account_without_data_is_absent_not_zero() {
        let o = ov();
        let (v, lines, legend) = o.view(0.0);
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
        let (v, lines, _) = o.view(0.0);
        assert!(v.cross.is_some());
        assert!(v.tooltip.contains("A  ") && v.tooltip.contains("B  50%"), "{}", v.tooltip);
        assert!(lines.iter().all(|l| l.dot_visible));
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
