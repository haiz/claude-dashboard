# Windows Sub-project 2: App Shell (Slint) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** A running Windows 11 desktop app (`claude-dashboard.exe`) with the macOS layout in Fluent/Mica dress: a tray icon showing peak usage, a Mica flyout popover, and a main window with a sidebar, a Dashboard card grid, and a per-account page — refreshing live from the accounts stored by sub-project 1.

**Architecture:** Pure Rust. Presentation logic (colors, ring geometry, countdown formatting, row sorting) ports from `apps/linux/lib/` into the `core` crate as tested modules. The GUI is a new `apps/windows/app` crate built on Slint (`fluent` style); it links `core` directly, runs a background refresh loop that feeds Slint models on the UI thread via `invoke_from_event_loop`, renders the tray ring with `tiny-skia`, and applies Mica with `window-vibrancy`. A named-pipe server receives `reload` from the sub-project-1 bridge.

**Tech Stack:** Rust 1.98 (pinned); `slint` (fluent style); `tray-icon`; `tiny-skia`; `window-vibrancy`; `windows-sys` (pipe server, window handle); `core` from `apps/linux/core`. Scope is build + run on `windows-latest`.

**Spec:** `docs/superpowers/specs/2026-10-04-windows-app-design.md` (sub-project 2 of 6).

**Prior sub-project:** sub-project 1 delivered `core` on Windows (store, DPAPI, cookie/win, `discover_windows_profiles_under`, `lock_store`, `key_intake`) and the bridge/extension. This sub-project consumes `core::store`, the pipe name `\\.\pipe\claude-dashboard`, and `extension-sources.json` (to mark extension-sourced accounts). Setup/Settings, charts, Command Log, and release are sub-projects 3–6 and are OUT OF SCOPE here.

## Global Constraints

- Toolchain 1.98.0 (pinned); the app crate is Windows-only (`apps/windows/app`), added to the existing `apps/windows` workspace.
- Look: the macOS layout and components (sidebar with colored tiles; account cards; ring gauges; green→red interpolation; per-account avatar colors; burn-rate animals), in Windows 11 materials — Mica background, Fluent controls, Segoe UI Variable, Segoe Fluent Icons. Light/dark follows the system setting and switches live.
- One source for colors/geometry: ported into `core` with tests asserting the SAME numbers as `apps/linux/lib/` (which was itself verified against macOS). Do not hand-tune duplicates in `.slint`.
- Windows 11 only; on Windows 10 the window falls back to a solid background (no Mica), no other Win10 work.
- Single instance: a named mutex; a second launch surfaces the running window and exits.
- The refresh loop never blocks the UI thread; results cross to it via `slint::invoke_from_event_loop`.
- Tray: left-click toggles the flyout; right-click shows Open Dashboard / Refresh / Quit.
- The app reads the store written by sub-project 1 and reloads on a `reload` pipe message; it never changes the account schema.

## Review Focus

1. **No accounts yet (first run / empty store):** the window and popover show an empty state with an "Add Account" affordance placeholder, not a blank or a crash. → Task 11, run check + `rows` empty-input test (Task 4).
2. **A refresh that errors for one account** keeps the last data for the others and shows that account's staleness/error, never a blank grid. → Task 6, test `refresh_merges_errors_without_dropping_rows`.
3. **The work-area / DPI the flyout anchors to** (taskbar on a non-default edge, scaled display): the popover stays on-screen next to the tray, not clipped off an edge. → Task 10, test `popover_origin_stays_within_work_area`.
4. **System theme flips while open:** colors and Mica follow without a restart. → Task 7, run check (documented).
5. **A second app launch** does not open a second window or a second tray icon. → Task 6, test `single_instance_guard_rejects_the_second_holder`.

## File / crate structure

```
apps/linux/core/src/
  colors.rs      (new)  hsv_to_rgb, usage_color, countdown_color, avatar_color, burn-rate animal/level (reuse burn_rate)
  geometry.rs    (new)  fill_fraction, progress_arc, countdown segments, ring metrics
  format.rs      (new)  formatted_countdown, format_reset_time
  rows.rs        (new)  build_rows: sort/tier accounts for display (contract "Sort order")
  lib.rs         (+ pub mod colors/geometry/format/rows)
apps/windows/
  Cargo.toml            (+ member "app")
  app/Cargo.toml
  app/build.rs          slint-build compiles ui/
  app/ui/app.slint      window root, theme tokens, sidebar, panes
  app/ui/components.slint  AccountCard, UsageGauge, UsageBar, Avatar, Sidebar row
  app/src/main.rs       single-instance guard, window + Mica, event loop
  app/src/refresh.rs    background refresh loop -> Slint models
  app/src/tray.rs       tray-icon + tiny-skia ring + menu
  app/src/popover.rs    borderless Mica flyout, work-area placement
  app/src/pipe.rs       named-pipe server for `reload`
  app/src/model.rs      Slint<->core bridge structs (row -> UI struct)
```

Pure logic (`core::colors/geometry/format/rows`) is unit-tested on every OS. The `apps/windows/app` crate is Windows-only and verified by building and running it (screenshots), since Slint views are not unit-testable; its non-UI helpers (popover placement, single-instance, refresh merge) ARE unit-tested.

Tasks are added below in dependency order. Pure-logic tasks (1–4) are TDD; GUI tasks (5, 7–11) are implement-build-run with the named non-UI helpers tested.

---

### Task 1: core::colors — usage/countdown/avatar colors (TDD)

**Files:** Create `apps/linux/core/src/colors.rs`; modify `apps/linux/core/src/lib.rs` (`pub mod colors;`).

**Port source:** `apps/linux/lib/colors.js` (verbatim math). SwiftUI `Color(hue:saturation:brightness:)` is HSV.

**Interfaces — produces:**
```rust
pub struct Rgb { pub r: f64, pub g: f64, pub b: f64 } // channels 0.0..=1.0
pub fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> Rgb
pub fn usage_color(utilization: f64) -> Rgb            // util 0..100, green->red
pub fn countdown_color(remaining_s: f64, total_s: f64) -> Rgb
pub fn avatar_color(seed: &str) -> Rgb                 // stable per-account hue from id/email
```
`avatar_color` mirrors the macOS per-account color (see `AccountBadgeColor.swift` / `AccountAvatar.swift`): derive a stable hue from a hash of `seed`, fixed saturation/brightness. Match the macOS derivation; if the Swift uses a specific hash, port that hash so the same account gets the same color across platforms.

- [ ] **Step 1: failing tests** — in `colors.rs`, assert against values computed from `colors.js`:
  - `usage_color(0.0)` is green-ish: hue 120/360, s 0.7, v 0.85 → assert the exact `hsv_to_rgb(120.0/360.0, 0.7, 0.85)` triple.
  - `usage_color(100.0)` is red: hue 0 → `hsv_to_rgb(0.0, 0.7, 0.85)`.
  - `usage_color(50.0)` equals `hsv_to_rgb(60.0/360.0, 0.7, 0.85)`.
  - `countdown_color(10.0, 100.0)` (fraction 0.1 ≤ 0.3) is the green-intensity branch; `countdown_color(50.0, 100.0)` (fraction 0.5 > 0.3) is `COUNTDOWN_BLUE` = (74,144,217)/255; `countdown_color(0.0, 100.0)` is the green fallback `hsv_to_rgb(120/360,0.7,0.85)`.
  - `avatar_color` is deterministic: same seed → same Rgb; two different seeds → (very likely) different hues.
  Use an epsilon compare helper (`(a-b).abs() < 1e-9`).

- [ ] **Step 2–4:** run (fails), implement the `colors.js` port verbatim plus `avatar_color`, run (passes).

- [ ] **Step 5:** `cargo test -p claude-dashboard-core colors` + `cargo clippy -p claude-dashboard-core --all-targets -- -D warnings`.

- [ ] **Step 6: commit** `feat(core): color interpolation ported for the Windows app`.

---

### Task 2: core::geometry — ring gauge geometry (TDD)

**Files:** Create `apps/linux/core/src/geometry.rs`; `lib.rs` `pub mod geometry;`.

**Port source:** `apps/linux/lib/geometry.js` (verbatim). Exposes ring fill, progress arc, and the countdown segment ranges the gauge draws.

**Interfaces — produces** (names mirror `geometry.js`):
```rust
pub const TAU: f64; pub const START_ANGLE: f64;
pub fn fill_fraction(utilization: f64) -> f64;
pub fn progress_arc(utilization: f64) -> (f64, f64);      // (start_angle, end_angle)
pub fn segment_count_for(total_seconds: f64) -> u32;
pub fn countdown_segments(remaining_s: f64, total_s: f64, diameter: f64, segment_count: u32) -> Vec<(f64, f64)>;
pub fn to_angle(fraction: f64) -> f64;
pub fn ring_radius(diameter: f64, line_width: f64) -> f64;
pub fn percent_font_size(diameter: f64) -> f64;           // + the other size helpers in geometry.js
```
Port `METRICS`/`METRICS_REGULAR`/`metrics_for(is_compact)` as a small struct too.

- [ ] **Step 1: failing tests** — pick the deterministic cases from `geometry.js`: `fill_fraction(0)==0`, `fill_fraction(100)==1`, `fill_fraction(50)==0.5`; `to_angle(0)==START_ANGLE`; `progress_arc(100)` spans a full turn from `START_ANGLE`; `segment_count_for` at the thresholds `geometry.js` encodes; `countdown_segments` returns `segment_count` ranges whose count of "filled" matches `remaining/total`. Assert against values read off `geometry.js`.
- [ ] **Step 2–4:** run-fail, port verbatim, run-pass.
- [ ] **Step 5:** test + clippy.
- [ ] **Step 6: commit** `feat(core): ring-gauge geometry ported for the Windows app`.

---

### Task 3: core::format — countdown & reset-time strings (TDD)

**Files:** Create `apps/linux/core/src/format.rs`; `lib.rs` `pub mod format;`.

**Port source:** `apps/linux/lib/format.js`.

**Interfaces — produces:**
```rust
pub fn formatted_countdown(remaining_s: f64, total_s: f64) -> String;
// reset-time formatting takes a timestamp; keep the signature testable without a clock:
pub fn format_reset_time(reset_unix_s: f64, total_s: f64, now_unix_s: f64) -> String;
```

- [ ] **Step 1: failing tests** — the deterministic cases from `format.js`: a sub-hour countdown renders `Xm` / `Mm Ss` as the source does; a multi-hour one renders `Hh Mm`; a non-positive remaining renders the source's zero form. For `format_reset_time`, pin one "today" and one "tomorrow" case with fixed `now`/`reset` seconds and assert the exact string the source would produce.
- [ ] **Step 2–4:** run-fail, port, run-pass. (ISO/locale: match `format.js` output exactly; no locale-dependent calls.)
- [ ] **Step 5:** test + clippy.
- [ ] **Step 6: commit** `feat(core): countdown/reset-time formatting ported for the Windows app`.

---

### Task 4: core::rows — display row building & sort order (TDD)

**Files:** Create `apps/linux/core/src/rows.rs`; `lib.rs` `pub mod rows;`.

**Port source:** `apps/linux/lib/model.js` `buildRows(...)` and `contract/README.md` "Sort order" (the binding rule). Reuse `core::burn_rate` and `core::usage` rather than re-deriving burn math.

**Interfaces — produces:**
```rust
pub struct DisplayRow {
    pub account_id: String,
    pub name: String,
    pub email: Option<String>,
    pub plan: AccountPlan,
    pub status: AccountStatus,
    pub five_hour: Option<WindowView>,   // utilization %, resets_at, is_limited
    pub seven_day: Option<WindowView>,
    pub fable: Option<WindowView>,
    pub peak_utilization: f64,           // drives the tray/menubar label
    pub burn_projected_seconds: Option<f64>,
    pub is_extension_sourced: bool,      // from extension-sources.json bindings
    pub error: Option<String>,
    pub last_synced_unix: Option<f64>,
}
pub struct WindowView { pub utilization: f64, pub resets_at_unix: Option<f64>, pub is_limited: bool }

pub struct BuildInput<'a> {
    pub accounts: &'a [Account],
    pub usage_by_account: &'a HashMap<String, UsageData>,  // or core's usage type
    pub errors: &'a HashMap<String, String>,
    pub extension_install_account_ids: &'a std::collections::HashSet<String>,
    pub now_unix_s: f64,
}
pub fn build_rows(input: BuildInput) -> Vec<DisplayRow>;
pub fn peak_utilization(rows: &[DisplayRow]) -> f64;      // highest, for the tray label
```
Sorting follows `contract/README.md` "Sort order" exactly (burn-rate tiers; the active-Claude-Code tier is populated in a later sub-project — here that signal is absent, same as the Linux daemon's current `null`). Port the tiering from `buildRows`.

- [ ] **Step 1: failing tests** — mirror `apps/linux/tests` row cases: empty input → empty vec (Review Focus 1); two accounts order by burn rate (higher burn first) per the contract; an account with an `errors` entry keeps its row with `error` set and is ordered per the rule; `is_extension_sourced` true iff its id is in `extension_install_account_ids`; `peak_utilization` returns the max across rows. Read the exact expected ordering from `contract/README.md` "Sort order" and `apps/linux/lib/model.js`.
- [ ] **Step 2–4:** run-fail, port `buildRows` + sort, run-pass.
- [ ] **Step 5:** test + clippy; also run the full `cargo test -p claude-dashboard-core` to confirm no regressions.
- [ ] **Step 6: commit** `feat(core): display-row building and sort order for the Windows app`.

---

### Task 5: app crate scaffold — a Slint window that builds and shows

**Files:** modify `apps/windows/Cargo.toml` (add member `app`); create `apps/windows/app/Cargo.toml`, `app/build.rs`, `app/ui/app.slint`, `app/src/main.rs`.

**Interfaces — produces:** a `claude-dashboard` binary that opens one Mica window titled "Claude Dashboard" with a placeholder body and theme tokens that follow the system light/dark setting. No data yet.

- [ ] **Step 1:** add deps to `app/Cargo.toml`: `slint = "1"`, build-dep `slint-build = "1"`, `claude-dashboard-core.workspace = true`, `window-vibrancy = "0.5"`, `windows-sys` (features for the window handle + DWM), `tray-icon`, `tiny-skia`, `image`/`raw-window-handle` as needed. `build.rs` = `slint_build::compile("ui/app.slint").unwrap();`. Pin matching versions; let the compiler/`cargo update` resolve.
- [ ] **Step 2:** `ui/app.slint` — an `export component AppWindow inherits Window` with `title: "Claude Dashboard"`, a `:root`-style palette via Slint `@theme`/global singletons holding the color tokens, Segoe UI Variable as the default font, min size, and a placeholder `Text`.
- [ ] **Step 3:** `main.rs` — `slint::include_modules!()`, create `AppWindow`, apply Mica via `window-vibrancy::apply_mica(hwnd, None)` (ignore the error on Windows 10 → solid background), `run()`. Get the HWND via `raw-window-handle` from the Slint window.
- [ ] **Step 4: build & run** — `cargo build -p claude-dashboard` then launch it; confirm a Mica window appears. Use the `run` skill / a screenshot. (No unit test; this is scaffolding.)
- [ ] **Step 5: commit** `feat(windows): Slint app window with Mica`.

Note: if `apply_mica` needs a specific `windows-sys`/`raw-window-handle` bridge, follow `window-vibrancy`'s documented recipe for Slint or winit; keep the fallback (solid background) when it returns Err.

---

### Task 6: refresh engine — background loop, single instance, reload pipe

**Files:** create `app/src/refresh.rs`, `app/src/pipe.rs`; modify `app/src/main.rs`.

**Interfaces — produces:**
```rust
// refresh.rs
pub struct RefreshOutput { pub rows: Vec<core::rows::DisplayRow>, pub peak: f64 }
pub fn refresh_once(now_unix_s: f64) -> RefreshOutput;   // load accounts, fetch usage per account in parallel, log, build_rows
pub fn merge_errors(prev: &[DisplayRow], fresh: Result<RefreshOutput, String>) -> RefreshOutput; // keep last good rows on partial failure
// pipe.rs
pub fn serve_reload<F: Fn() + Send + 'static>(on_reload: F);  // spawns a thread serving \\.\pipe\claude-dashboard
// main.rs
fn acquire_single_instance() -> Option<SingleInstance>;  // named mutex; None if already held
```
The loop runs on a background thread (interval from settings later; a fixed default here), calls `refresh_once`, and hands `RefreshOutput` to the UI via `slint::invoke_from_event_loop`. A failed whole refresh keeps the previous rows (`merge_errors`); a per-account error rides in that row's `error` (already from `build_rows`). The pipe server calls back to trigger an immediate refresh on `reload`.

- [ ] **Step 1: failing tests** (non-UI helpers, pure):
  - `merge_errors`: given previous rows and a `fresh: Err(..)`, returns the previous rows unchanged (Review Focus 2); given `Ok`, returns the fresh ones.
  - `refresh_merges_errors_without_dropping_rows`: build an `Ok(RefreshOutput)` where one row carries an `error` and others do not; assert the row count is preserved and the errored row keeps its last utilization. (Use `build_rows` with an `errors` entry.)
  - `single_instance_guard_rejects_the_second_holder`: acquire once → `Some`; a second acquire while the first is held → `None`; after dropping the first → `Some` again. (Named mutex via `windows-sys`; `#[cfg(windows)]`.)
- [ ] **Step 2–4:** run-fail, implement (`refresh_once` uses `core::store::load_accounts`, `core::api::usage_raw` per account on a small thread pool or sequential loop, `core::store::UsageLogStore::record`, `core::rows::build_rows`; read `extension-sources.json` via the bridge's `sources` shape or a local reader for `is_extension_sourced`), run-pass.
- [ ] **Step 5:** wire into `main.rs`: acquire single instance (on `None`, signal the running instance to show and exit — a second pipe message or a `SetForegroundWindow`), start the loop thread, start the pipe server. Build & run; confirm live rows appear once Task 8 lands (here just confirm it compiles and the loop logs).
- [ ] **Step 6: commit** `feat(windows): refresh loop, single-instance guard, reload pipe`.

---

### Task 7: main window shell — sidebar + pane switching

**Files:** `app/ui/app.slint` (expand), `app/ui/components.slint` (new, sidebar row), `app/src/model.rs` (Slint<->core structs).

**Interfaces — produces:** a `NavigationView`-style sidebar with the four groups and colored tiles matching macOS `SidebarItem.Style` (Dashboard=blue, Overview=orange — Overview is a later sub-project so show it disabled/placeholder; Accounts group lists one row per account; Tools=Command Log/Help placeholders; Settings=Accounts/General placeholders), and a content area that swaps panes by a `selection` property. Custom title bar area holds the pane title (Mica extends up).

- [ ] **Step 1:** `model.rs` — a Slint struct `UiRow` (fields the views need: name, email, plan string, status, peak, per-window percents, colors as Slint `brush`, avatar color, animal, staleness text, error) and `fn to_ui_row(&DisplayRow) -> UiRow` using `core::colors/format`. A `VecModel<UiRow>` is set on the window.
- [ ] **Step 2:** `components.slint` sidebar row component (tile color, Segoe Fluent icon glyph, label). `app.slint` sidebar listing the static items + an account list bound to the model; `selection` enum drives the content area.
- [ ] **Step 3: build & run** — confirm the sidebar renders with tiles and selecting a row switches the (still placeholder) content. Screenshot. Theme flip check (Review Focus 4): toggle Windows dark mode, confirm colors follow (documented run check).
- [ ] **Step 4: commit** `feat(windows): main window sidebar and pane switching`.

---

### Task 8: Dashboard pane — card grid, ring gauge, usage bar, avatar

**Files:** `app/ui/components.slint` (AccountCard, UsageGauge, UsageBar, Avatar), `app/ui/app.slint` (Dashboard pane binds the grid).

**Interfaces — produces:** the Dashboard pane: a responsive grid of `AccountCard`s, each with the avatar (per-account color), name/plan, a `UsageGauge` ring (drawn with Slint `Path` using `core::geometry` arcs and `core::colors`), `UsageBar`s for 5h/7d (green→red), a reset countdown, and the burn-rate animal. Animate gauge value changes; highlight card on hover.

- [ ] **Step 1:** build the components in `.slint`, driven entirely by `UiRow` fields (the Rust side already computed colors/arcs/animal/countdown in Task 7's `to_ui_row` + Tasks 1–3). The gauge `Path.commands` string is built in Rust from `core::geometry::progress_arc`/`countdown_segments` and passed as a `UiRow` field, OR computed in `.slint` from numeric fields — prefer Rust-built path strings so geometry stays single-sourced and tested.
- [ ] **Step 2: build & run** with real accounts in the store (from sub-project 1 paste-key or a hand-seeded `accounts.json` + a mock usage). Confirm cards render with correct colors and gauges. Screenshot. Compare side-by-side with a macOS screenshot for layout parity.
- [ ] **Step 3: commit** `feat(windows): dashboard card grid with ring gauges`.

---

### Task 9: per-account pane

**Files:** `app/ui/app.slint` (AccountPane), `app/ui/components.slint` (gauge row reuse).

**Interfaces — produces:** the per-account page selected from the sidebar: header (avatar, name, email, plan, status), a `UsageGaugeRow` (the 5h/7d/Fable gauges larger), and action buttons placeholders (Re-sync this account is wired; chart and Command Log are later sub-projects, shown disabled). Mirrors macOS `AccountPane`.

- [ ] **Step 1:** build the pane bound to the selected `UiRow`; "Re-sync" triggers an immediate `refresh_once` for that account (or all) via a callback into the refresh engine.
- [ ] **Step 2: build & run** — select an account, confirm the page renders and Re-sync updates it. Screenshot.
- [ ] **Step 3: commit** `feat(windows): per-account page`.

---

### Task 10: tray icon + Mica flyout popover

**Files:** `app/src/tray.rs`, `app/src/popover.rs`, `app/ui/app.slint` (a `PopoverWindow` component).

**Interfaces — produces:**
```rust
// tray.rs
pub fn render_tray_icon(peak_utilization: f64) -> image::RgbaImage; // tiny-skia ring filled to peak, green->red
pub fn install_tray(...) -> TrayHandle;  // left-click -> toggle popover; right-click -> Open Dashboard/Refresh/Quit
// popover.rs
pub fn popover_origin(tray_rect: Rect, popover_size: Size, work_area: Rect) -> Point; // clamp on-screen next to the tray
```
The popover is a borderless, rounded, Mica `PopoverWindow` (header + account rows with small gauges + expand button) shown at `popover_origin`, auto-hidden on focus loss.

- [ ] **Step 1: failing tests** (pure):
  - `popover_origin_stays_within_work_area` (Review Focus 3): a tray rect in each corner + a work area smaller than screen → the returned origin keeps the whole popover inside `work_area` (right/bottom edges clamped, never off-screen).
  - `render_tray_icon` is deterministic and non-empty: same peak → identical bytes; `peak=0` vs `peak=100` differ (color). Assert dimensions and that it is not all-transparent.
- [ ] **Step 2–4:** run-fail, implement `popover_origin` (clamp math) and `render_tray_icon` (tiny-skia ring via `core::geometry`+`core::colors`), run-pass.
- [ ] **Step 5:** install the tray (`tray-icon`), wire left/right click, build the `PopoverWindow`, place it with `popover_origin`, apply Mica, hide on focus-out. Build & run; confirm the tray ring reflects peak, left-click shows the flyout at the tray, right-click shows the menu. Screenshots.
- [ ] **Step 6: commit** `feat(windows): tray ring icon and Mica flyout popover`.

---

### Task 11: wire it together + empty state + run

**Files:** `app/src/main.rs`, `app/ui/app.slint`.

**Interfaces — produces:** the refresh loop's `RefreshOutput` drives the window model, the tray icon (peak), and the popover, all via `invoke_from_event_loop`. Empty store shows an empty state with an "Add Account" placeholder button (wired in sub-project 3) in both the window and the popover (Review Focus 1). Per-account errors/staleness render on the card.

- [ ] **Step 1:** connect refresh → `VecModel<UiRow>` + tray redraw + popover model; on `reload` pipe message, refresh immediately; menu "Refresh" and "Quit" work; "Open Dashboard" shows the main window.
- [ ] **Step 2:** empty state in the Dashboard pane and popover when the model is empty.
- [ ] **Step 3: build & run end-to-end** — with (a) an empty store: empty state shows; (b) a seeded store with ≥2 accounts and mock usage: cards, gauges, tray ring, popover all reflect it; trigger a `reload` via the bridge pipe and confirm the app refreshes. Screenshots of each. This is the sub-project's acceptance check.
- [ ] **Step 4: commit** `feat(windows): wire refresh to window, tray and popover; empty state`.

---

### Task 12: CI + docs

**Files:** `.github/workflows/ci.yml` (extend the windows job), `CLAUDE.md`, `contract/windows.md` (a short "App shell" note if any new cross-process behaviour, e.g. the single-instance mutex name and the reload-pipe consumer).

- [ ] **Step 1:** add to the windows CI job: `cd apps/windows && cargo build -p claude-dashboard` and `cargo test -p claude-dashboard` (the non-UI helper tests). Keep the existing core/bridge/extension steps. Note GUI rendering is not covered by CI (verified by the run checks above).
- [ ] **Step 2:** `CLAUDE.md` — extend the Windows subsection with `apps/windows/app` (Slint UI, refresh loop, tray, popover) and the new `core` presentation modules (`colors`, `geometry`, `format`, `rows`). Add the build/run command.
- [ ] **Step 3:** `contract/windows.md` — document the single-instance mutex name and that the app is the `reload` pipe's consumer (sub-project 1 is the writer).
- [ ] **Step 4: commit** `docs(windows): CI build and docs for the app shell`.

---

## Done when

- `core::colors/geometry/format/rows` are ported with tests asserting the same values as `apps/linux/lib/`, and the full `cargo test -p claude-dashboard-core` is green on Windows and (via CI) Linux.
- `cargo build -p claude-dashboard` succeeds and `cargo test -p claude-dashboard` (non-UI helpers: `merge_errors`, single-instance, `popover_origin`, `render_tray_icon`) passes.
- A run check shows: a Mica window with the sidebar, a Dashboard card grid with correct ring gauges and colors, a per-account page, a tray ring reflecting peak usage, a Mica flyout at the tray, light/dark following the system, and the empty state when there are no accounts — with screenshots.

Sub-project 3 (Setup & Settings) consumes: the sidebar's Settings panes placeholders, the "Add Account" affordances, `core::store` + `cookie::win` + `discover_windows_profiles_under` (scan tab), `key_intake` (paste tab), and the extension install flow.
