# Windows Sub-project 4: Charts — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Interactive usage charts in the Windows app: an Overview chart (one line per account over a chosen window) and a per-account chart, both over the usage-log time series, with range presets (5h/24h/3d/7d/30d), a hover crosshair + value tooltip, drag-to-zoom, and double-click-to-reset. The per-account page's disabled "View chart" button and the sidebar's Overview item become real.

**Architecture:** Pure Rust. New testable `core` pieces: a usage-log read/series API on `UsageLogStore`, and a `core::chart` module porting the chart math from `apps/linux/lib/chart.js` (scale, ticks, nearest-point, segments, zoom/pan range, full range, total series). The `apps/windows/app` crate renders the chart in Slint from Rust-built Path command strings (geometry single-sourced through `core::chart`), handles hover/zoom/reset, and reuses `core::colors` (avatar colors for Overview lines). No schema change; the usage log is already written each refresh (sub-project 2).

**Tech Stack:** Rust 1.98 (pinned); `slint`; `core` from `apps/linux/core` (rusqlite usage-log). Build + run on `windows-latest`.

**Spec:** `docs/superpowers/specs/2026-10-04-windows-app-design.md` (sub-project 4 of 6).

**Prior sub-projects (reuse):**
- `core::store::UsageLogStore` (SQLite usage log: `record`/`record_at`/`count`/`raw_u`; schema `usage_logs(aid,w,rat,t,u,lim)` + `accounts_map(aid,account_id)`; `u` stored as round(utilization*100), `t`/`rat` are unix seconds, `w` is the window code 0=5h,1=7d,3=Fable) and `usage_log_path()`.
- `core::colors` (avatar_color, usage_color), `core::rows` (DisplayRow, window codes), `core::geometry` (if useful).
- App: refresh loop already records usage each cycle; the per-account pane (`AccountPane`) has a DISABLED "View chart" button; the sidebar has a DISABLED "Overview" item; `selection`/Pane enum; `to_ui_row`; theme tokens; the main window + panes.
- Reference the macOS views for layout/behaviour: `apps/macos/ClaudeDashboard/Views/OverviewChartView.swift`, `AccountDetailView.swift`, `InteractiveChartContainer.swift`, and the port target `apps/linux/lib/chart.js`.

Out of scope (keep the first pass bounded; note as fast-follow): the macOS "measure tool" (two-point delta); the reset-cycle markers overlay; CSV export. Charts show the time series + crosshair + zoom/reset only. Command Log is sub-project 5; release is 6.

## Global Constraints

- Toolchain 1.98.0 (pinned); app crate Windows-only; `core` additions plain Rust (rusqlite already a core dep; no cfg needed for the series read or chart math).
- Chart geometry/scale/ticks/nearest-point come from `core::chart` (ported from `apps/linux/lib/chart.js`, SAME values) — no scale/tick/hit-test math hand-written in `.slint` or duplicated in the app. Colors from `core::colors`.
- The usage-log `u` column is `round(utilization*100)`; a series read returns utilization as `u as f64 / 100.0` (0..100). `t`/`rat` are unix seconds; the chart works in milliseconds like `chart.js` (convert at the read/boundary).
- Windows: `w` codes are 5h=0, 7d=1, Fable=3 (per `contract/usage-log.md` / `core::rows`); the chart's window selector maps presets/windows to these codes.
- Reads are read-only (no lock needed); the chart loads off the UI thread and hands data to Slint via `invoke_from_event_loop`.
- No account-schema or usage-log-schema change.

## Review Focus

1. **An account/window with no logged points** (new account, or a window never limited) renders an empty chart with "no data yet", not a crash or a divide-by-zero in the scale. → Task 2 test `scale_and_fullrange_handle_empty`; Task 4 run-check.
2. **Zoom clamps**: drag-zoom and the preset buttons never produce an inverted or sub-minimum/over-maximum range (zoomRange min/max span bounds), and a double-click resets to the full/preset range. → Task 2 test `zoom_range_clamps_min_and_max`.
3. **Hover maps to the nearest real sample** (not an interpolated x), and the tooltip shows that sample's time + value; hovering outside the plotted area shows nothing. → Task 2 test `nearest_entry_picks_closest`; Task 3 test for x↔time mapping.
4. **A gap in the series** (missing polls / a reset) is not drawn as a straight line across the gap — segments break the line where samples are far apart, matching `chart.js::segments`. → Task 2 test `segments_split_on_gap`.
5. **Overview total/line scaling**: multiple accounts over one window scale to the same axes; an account with no data in the window is simply absent, not a flat-zero line that implies 0%. → Task 5 run-check + Task 2 `total_series` test.

## File / crate structure

```
apps/linux/core/src/
  store.rs    (+ UsageLogStore::series / series_all read methods; SeriesPoint)
  chart.rs    (new) port of chart.js: Scale, make_scale, time_ticks, Y_TICKS, nearest_entry, segments, zoom_range, pan_range, full_range, total_series, measure
  lib.rs      (+ pub mod chart)
apps/windows/app/src/
  chart_model.rs (new) build Slint Path command strings + tick/label positions + hover hit-test from core::chart (pure parts tested); range-preset -> (window code, span) mapping
  chart.rs       (new) load series off-thread, interaction state (range, hover, zoom), feed the chart component
  ui/chart.slint (new) a reusable InteractiveChart component (line Path, axes, grid, crosshair, tooltip, range buttons) + OverviewChart (multi-line) usage
  (wire) per-account "View chart" -> account chart; sidebar Overview -> Overview pane
contract/usage-log.md or windows.md (+ note the series read + window codes if not already documented)
CLAUDE.md (+ core::chart, the chart UI)
```

Tasks below in dependency order. TDD for the series read + chart math + the pure chart-model helpers; GUI tasks (chart rendering + interaction) are build-run + the named pure helpers, verified by controller run-checks (screenshots).

---

### Task 1: UsageLogStore series read (TDD)

**Files:** Modify `apps/linux/core/src/store.rs` (add read methods + a SeriesPoint type near UsageLogStore).

**Interfaces — produces:**
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeriesPoint { pub t_unix: f64, pub utilization: f64 }  // utilization = u/100.0 (0..100)
impl UsageLogStore {
    /// Points for one account+window within [from_unix, to_unix], ordered by time ascending.
    pub fn series(&self, account_id: &str, window: i64, from_unix: f64, to_unix: f64) -> Vec<SeriesPoint>;
    /// Points per account for one window within the range: account_id -> Vec<SeriesPoint> (for Overview).
    pub fn series_all(&self, window: i64, from_unix: f64, to_unix: f64) -> std::collections::HashMap<String, Vec<SeriesPoint>>;
}
```
`series`: JOIN accounts_map, WHERE account_id=? AND w=? AND t BETWEEN from AND to, ORDER BY t ASC; map `u` -> `u as f64 / 100.0`, `t` -> `t as f64`. `series_all`: same but grouped by account_id across all accounts for that window.

- [ ] **Step 1: failing tests** (use `UsageLogStore::open_in_memory()`):
  - record a few points via `record_at` for ACC/window 0 at increasing t; `series("ACC",0, from, to)` returns them in ascending t with utilization == the recorded value (respecting the round(*100) encoding — record 42.0 -> read 42.0; record 45.005 -> read 45.01).
  - range filter: points outside [from,to] are excluded; boundary inclusive.
  - a different window (1) or account is not returned.
  - `series` for an unknown account/window -> empty vec (Review Focus 1 data path).
  - `series_all(0, from, to)` groups points by account for two accounts.
- [ ] **Step 2-4:** run-fail, implement (rusqlite prepared statements), run-pass.
- [ ] **Step 5:** `cargo test -p claude-dashboard-core` (full) + `cargo clippy -p claude-dashboard-core --all-targets -- -D warnings`.
- [ ] **Step 6: commit** `feat(core): usage-log series read for charts`.

---

### Task 2: core::chart — port chart.js math (TDD)

**Files:** Create `apps/linux/core/src/chart.rs`; `lib.rs` `pub mod chart;`.

**Port source:** `apps/linux/lib/chart.js` (VERBATIM math). Work in milliseconds like the source.

**Interfaces — produces** (names mirror chart.js):
```rust
pub const Y_MIN: f64 = 0.0; pub const Y_MAX: f64 = 100.0;
pub const Y_TICKS: [f64; 5] = [0.0,25.0,50.0,75.0,100.0];
pub struct Range { pub from_ms: f64, pub to_ms: f64 }
pub struct Scale { /* fields from makeScale: maps (ms,value)->(x,y) within width/height/padding */ }
pub fn make_scale(range: Range, width: f64, height: f64, padding: f64) -> Scale;
impl Scale { pub fn x(&self, ms: f64) -> f64; pub fn y(&self, value: f64) -> f64; }   // match makeScale's returned mappers
pub struct Tick { pub ms: f64, pub label: String }
pub fn time_ticks(from_ms: f64, to_ms: f64, max_ticks: usize) -> Vec<f64>;
pub fn format_tick(ms: f64, span_ms: f64) -> String;   // match chart.js formatTick (locale-free; match the Linux-pinned form)
pub struct Entry { pub ms: f64, pub value: f64 }
pub fn nearest_entry(entries: &[Entry], ms: f64) -> Option<usize>;   // index of nearest by |ms-entry.ms|
pub fn segments(entries: &[Entry]) -> Vec<Vec<Entry>>;               // split runs on a gap, per chart.js
pub fn zoom_range(range: Range, factor: f64, anchor_ms: f64) -> Range; // clamp [min_span, max_span] like chart.js
pub fn pan_range(range: Range, delta_ms: f64) -> Range;
pub fn full_range(entries: &[Entry], now_ms: f64) -> Range;
pub fn total_series(by_account: &std::collections::HashMap<String, Vec<Entry>>) -> Vec<Entry>;
pub fn measure(a: (f64,f64), b: (f64,f64)) -> (f64,f64);
```
Port the exact constants from chart.js: zoom min span 60_000 ms, max span 90*86_400_000 ms; the segment gap rule; the tick spacing. For `format_tick`, reproduce the SAME concrete strings the Linux side produces (check `apps/linux/tests` for pinned expectations; no locale-dependent calls — compute h:mm / day labels explicitly).

- [ ] **Step 1: failing tests** — mirror `apps/linux/tests` chart cases + Review Focus:
  - `scale_and_fullrange_handle_empty` (RF1): `make_scale` with from==to or zero width doesn't panic/NaN; `full_range(&[], now)` returns a sane default span (match chart.js fullRange's empty branch).
  - `zoom_range_clamps_min_and_max` (RF2): zooming in past 60s clamps to 60s; out past 90d clamps to 90d; anchor is preserved; never inverted.
  - `nearest_entry_picks_closest` (RF3): midpoints and exact hits pick the right index; empty -> None.
  - `segments_split_on_gap` (RF4): a gap larger than the threshold splits into two segments; contiguous points stay one.
  - `total_series` (RF5): sums/merges per-account entries as chart.js totalSeries does.
  - `make_scale` maps a known (ms,value) to the expected (x,y) with padding (literal expected numbers).
- [ ] **Step 2-4:** run-fail, port verbatim, run-pass.
- [ ] **Step 5:** test + full core test + clippy.
- [ ] **Step 6: commit** `feat(core): chart math (scale, ticks, zoom, segments) ported for Windows`.

---

### Task 3: app chart_model — Slint path/ticks/hit-test from core::chart (TDD the pure parts)

**Files:** Create `apps/windows/app/src/chart_model.rs`.

**Interfaces — produces:**
```rust
pub struct RangePreset; // 5h/24h/3d/7d/30d -> (window_code: i64, span_ms: f64)
pub fn preset_window_and_span(preset: &str) -> (i64, f64);
/// Build a Slint Path commands string for one series within a Scale (polyline per core::chart::segments; gaps break the path).
pub fn series_path(entries: &[core::chart::Entry], scale: &core::chart::Scale) -> String;
/// Y-gridline + x-tick pixel positions + labels for the axes.
pub fn axis_ticks(range: core::chart::Range, scale: &core::chart::Scale) -> (Vec<(f64,String)> /*x*/, Vec<(f64,f64)> /*y value,px*/);
/// Map a pointer x (px) back to a time (ms) within the plot, for hover.
pub fn x_to_ms(x_px: f64, scale: &core::chart::Scale, range: core::chart::Range) -> f64;
```

- [ ] **Step 1: failing tests**:
  - `preset_window_and_span` maps "5h"->(0, 5h-ms), "7d"->(1, 7d-ms), "30d"->(1, 30d-ms over the 7d window code), etc. (map each preset to the right window code + span; document the 24h/3d/30d-over-which-window choice, mirroring macOS TimeRangePreset).
  - `series_path` on a 2-point contiguous series starts with "M " and has one "L "; a series with a gap (via core::chart::segments) produces two "M " subpaths (the line breaks — Review Focus 4 at the render level).
  - `x_to_ms` is the inverse of `scale.x` within rounding (RF3 mapping): x_to_ms(scale.x(ms)) ≈ ms.
  - empty entries -> empty path string (no panic) (RF1).
- [ ] **Step 2-4:** run-fail, implement using core::chart (do NOT re-derive scale/segments), run-pass.
- [ ] **Step 5:** `cargo test -p claude-dashboard` + build + clippy.
- [ ] **Step 6: commit** `feat(windows): chart path/tick/hit-test model from core::chart`.

---

### Task 4: per-account interactive chart UI

**Files:** Create `apps/windows/app/ui/chart.slint` (InteractiveChart component); `apps/windows/app/src/chart.rs` (load series off-thread + interaction state); wire the AccountPane "View chart" button to show the chart.

**Reference:** macOS `AccountDetailView.swift` + `InteractiveChartContainer.swift`.

**Interfaces — produces:** an `InteractiveChart` Slint component driven by Rust-provided fields (line path(s) string, axis ticks, crosshair x + tooltip text, range-button state). `chart.rs` loads `UsageLogStore::series(account, window, from, to)` on a worker thread (via `invoke_from_event_loop`), holds interaction state (current Range, hover ms, which preset), and recomputes the path via chart_model when the range/hover changes.

- [ ] **Step 1:** build the component + wiring:
  - range preset buttons (5h/24h/3d/7d/30d) set the window+range (preset_window_and_span) and reload the series.
  - the line is drawn from `series_path` (segments break gaps); Y gridlines at Y_TICKS; X ticks from axis_ticks; the usage color/account avatar color for the line.
  - hover: on pointer move, x_to_ms -> nearest_entry -> draw a vertical crosshair at that sample and a tooltip (time + "NN%"); hovering off-plot hides it (RF3).
  - drag to zoom: a horizontal drag selects a sub-range -> zoom_range/or set range to the selection (clamped); double-click resets to the preset's full range (RF2).
  - empty series -> "No data yet" placeholder (RF1), no crosshair.
- [ ] **Step 2:** wire the per-account pane: enable the "View chart" button (SP2 left it disabled) to reveal the chart for the selected account (inline in the pane or a sub-view).
- [ ] **Step 3: build & run** — `cargo build -p claude-dashboard` clean; clippy clean; `cargo test -p claude-dashboard` green; `cd ../linux && cargo test -p claude-dashboard-core` green. Smoke (SMOKE, and SMOKE+FAKE_ROWS) exits 0. Controller run-check: a chart renders with axes; hovering shows the crosshair+tooltip; drag zooms; double-click resets; an empty account shows "No data yet". (FAKE_ROWS has no logged history, so seed a few UsageLogStore points behind an env flag, OR note the chart will be empty under FAKE_ROWS and run-check with a real logged account.)
- [ ] **Step 4: commit** `feat(windows): per-account interactive usage chart`.

---

### Task 5: Overview chart (multi-account) + sidebar wiring

**Files:** extend `ui/chart.slint` (Overview usage) + `src/chart.rs` (series_all, account toggles, window selector); enable + route the sidebar "Overview" item to an Overview pane.

**Reference:** macOS `OverviewChartView.swift`.

**Interfaces — produces:** an Overview pane: a window selector (5h/7d/Fable), a range preset, a legend/toggle list of accounts (each in its avatar color), and one line per selected account over the window (series_all + series_path each, same Scale), sharing axes; hover shows each account's nearest value (or a combined tooltip). An account with no data in the window is simply absent (RF5).

- [ ] **Step 1:** build the Overview pane reusing the InteractiveChart component for the plot area; load `series_all(window, from, to)` off-thread; draw one path per account in `core::colors::avatar_color`; account toggle checkboxes; window + range selectors; same zoom/hover/reset interactions.
- [ ] **Step 2:** enable the sidebar "Overview" item (remove enabled:false, add `selected`/`clicked => selection = Pane.overview`, mirroring the Accounts/General fix) and route it to the Overview pane. VERIFY the edit landed by reading the row back (the sidebar-enable edit failed silently twice in SP3).
- [ ] **Step 3: build & run** — build/clippy/tests green; smoke exits 0. Controller run-check: Overview renders multiple colored lines, toggling an account shows/hides its line, the window/range selectors work, an account with no data is absent (not a flat-zero line).
- [ ] **Step 4: commit** `feat(windows): Overview multi-account chart`.

---

### Task 6: CI, docs, contract

**Files:** `.github/workflows/ci.yml` (confirm existing steps cover core::chart + the series read + app tests — likely no change); `CLAUDE.md`; `contract/usage-log.md` or `contract/windows.md`.

- [ ] **Step 1:** CLAUDE.md — add `core::chart` and the UsageLogStore series read to the Windows/core notes; note the Overview + per-account charts under apps/windows/app.
- [ ] **Step 2:** contract — document the series read semantics (u/100 decode, window codes, ascending order) in `contract/usage-log.md` (it already defines the schema) and that the chart math is shared via core::chart (ported from lib/chart.js). Keep brief.
- [ ] **Step 3:** confirm green: `cd apps/linux && cargo test -p claude-dashboard-core`; `cd ../windows && cargo test -p claude-dashboard && cargo clippy --all-targets -- -D warnings`. No BOM introduced (bash/Edit, not Set-Content).
- [ ] **Step 4: commit** `docs(windows): chart math + usage-log series read docs`.

---

## Done when

- `UsageLogStore::series`/`series_all` and `core::chart` (ported from chart.js) have passing tests; `cargo test -p claude-dashboard-core` green on Windows and (via CI) Linux.
- `chart_model` pure helpers tested; `cargo test -p claude-dashboard` green.
- A controller/user run-check shows: a per-account chart (axes, line, hover crosshair+tooltip, drag-zoom, double-click reset, "No data yet" when empty) and an Overview chart (multi-account colored lines, account toggles, window/range selectors) — with screenshots. The per-account "View chart" button and the sidebar "Overview" item are reachable.

Sub-project 5 (Command Log) consumes: the Tools sidebar placeholders, and ports CommandRunner/CommandClassifier/TerminalLauncher with a Windows Job Object + Windows Terminal launch.
