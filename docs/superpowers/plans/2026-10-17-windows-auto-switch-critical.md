# Windows Auto Switch + Critical Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port macOS Auto Switch, Switch Log, Critical accounts and the X ratio to the Windows app, identical in rules, texts and UI.

**Architecture:** Pure rules and SQLite stores live in `apps/linux/core` (`auto_switch`, `auto_switch_log`, `critical`, `usage_ratio`, new settings keys). The app owns a global controller evaluated on the refresh thread after each refresh, a fast-poll timer in the refresh loop, an hourly X thread, and the Slint UI.

**Tech Stack:** Rust 2021, rusqlite, chrono + chrono-tz (new, core), Slint 1.x.

**Spec:** `docs/superpowers/specs/2026-10-17-windows-auto-switch-critical-design.md` (and the macOS sources it names; macOS code is the authority for every constant and text).

## Global Constraints

- Every constant, default, range, text, tooltip and Help sentence is copied from the macOS source named in the spec.
- Settings keys use the macOS names: `autoSwitchEnabled` (default true), `autoSwitchFiveHourThreshold` (95, 50...99), `autoSwitchSevenDayThreshold` (99, 50...100), `autoSwitchShowWidgetToggle` (true), `autoSwitchPriorityAccountId`, `autoSwitchPriorityBaseline` (Unix seconds), `criticalPace` (50, 10...100), `criticalHighPace` (80, >= pace), `criticalQuietStart` (0, 0...23), `criticalQuietEnd` (7, 0...23), `criticalTimeZone` ("Asia/Ho_Chi_Minh", must parse).
- Dev runs (`CLAUDE_DASHBOARD_FAKE_ROWS=1`) never switch Claude Code (no switcher).
- No UI-thread blocking: switching happens on the refresh thread.
- Commits: `git -c user.name=engineering -c user.email=engineering@gotitapp.co commit`.

## Review Focus

- An auto switch racing a manual Switch: both must never run at once (shared busy flag).
- A failed fetch of the active account: no decision on stale numbers (`is_fresh = error.is_none()`).
- Home account deleted: the tick is dropped on the next evaluation.
- Settings file holding out-of-range thresholds or an unknown time zone: fall back to defaults.
- Fast poll must stop when auto switch is turned off or the active account leaves the warning zone.

---

### Task 1: Settings keys (core)

**Files:** Modify `apps/linux/core/src/settings.rs`.

**Produces:** fields on `Settings`: `auto_switch_enabled: bool`, `auto_switch_five_hour: i64`, `auto_switch_seven_day: i64`, `auto_switch_show_widget_toggle: bool`, `auto_switch_priority_id: Option<String>`, `auto_switch_priority_baseline: Option<f64>`, `critical_pace: i64`, `critical_high_pace: i64`, `critical_quiet_start: i64`, `critical_quiet_end: i64`, `critical_time_zone: String`. Out-of-range stored values load as defaults; high pace loads as `max(stored, pace)`; unknown zone loads as default. `reset_general` also resets these (priority kept).

- [ ] Tests: `auto_switch_and_critical_defaults`, `out_of_range_values_fall_back`, `high_pace_never_below_pace`, `reset_general_keeps_home_account`, `round_trip_keys_use_macos_names`. RED, implement, GREEN, commit.

### Task 2: Critical planner (core)

**Files:** Create `apps/linux/core/src/critical.rs`; add `chrono`, `chrono-tz` to core Cargo.toml.

**Produces:** `QuietHours { start_hour, end_hour, tz: chrono_tz::Tz }`, `CriticalLevel { None, Critical, High }` with `label()`, `CriticalAssessment { level, waste }`, `CriticalParameters { x, pace, high_pace, quiet: Option<QuietHours> }`, `active_hours(start_unix, end_unix, quiet) -> f64`, `projected(...)`, `assess(seven_day: Option<(f64, Option<f64>)>, params, now_unix) -> CriticalAssessment`, `badge(&CriticalAssessment, &CriticalParameters) -> Option<String>` (help text), `parameters_from(&Settings, x) -> CriticalParameters`, `time_zone_names() -> Vec<&'static str>` sorted.

- [ ] Tests (from macOS `CriticalPlannerTests`): quiet across midnight, other zone, DST day, start == end, 7d not started, exactly 100 is critical, high before critical, 7d >= 99 none, badge texts. RED, GREEN, commit.

### Task 3: X estimator and ratio store (core)

**Files:** Create `apps/linux/core/src/usage_ratio.rs`; add `ratio_log_path()` and `auto_switch_log_path()` to `store.rs`.

**Produces:** `LogRow { account_id, t_unix, utilization, resets_at_unix }`, `estimate(five: &[LogRow], seven: &[LogRow]) -> Option<Estimate { x, sum_five, sum_seven }>`, constants (`LOOKBACK_S = 3 days`, `SEVEN_DAY_LOOKBACK_S = 7 days`, `DEFAULT_X = 16.0`, `RETENTION_S = 90 days`), `RatioStore::{open, open_in_memory, insert(RatioPoint), latest(), series(from, to), delete_older_than}` on `ratio_logs(id, t, x, d5, d7)`.

- [ ] Tests: reset inside window skipped, delta < 10 skipped, not enough data -> None, several accounts summed, sparse 7d rows read by step, negative 7d skipped; store insert/latest/series/retention. RED, GREEN, commit.

### Task 4: Auto switch planner (core)

**Files:** Create `apps/linux/core/src/auto_switch.rs`.

**Produces:** `Usage { five: f64, five_resets: Option<f64>, seven: f64, seven_resets: Option<f64> }`, `Account { id, display_name, usage: Option<Usage>, ready: bool, is_usable, is_active, is_fresh, critical: CriticalAssessment, five_hour_log: Vec<(f64 t, f64 u, f64 resets)> }`, `Thresholds`, `Priority { account_id, baseline: Option<f64> }`, `Lock`, `Hold`, `Snapshot`, `Reason { PriorityReset, Critical(CriticalLevel), Threshold(u8) }`, `StayReason`, `Decision`, and the macOS functions `is_triggered`, `is_in_warning_zone`, `is_in_toggle_zone`, `score`, `has_reset`, `priority_baseline`, `is_priority_due`, `effective_thresholds`, `make_lock`, `updated_lock`, `is_candidate`, `is_critical_target`, `waits_for_reset`, `decide(&Snapshot, pick_index: impl FnMut(usize) -> usize)`.

- [ ] Tests: one per macOS branch (high pulls from home; critical does not pull from home; home pulls from critical, not from high; critical does not pull from critical; 5h 3% target not eligible; largest waste wins; held account stays; expired hold ignored; thresholds fire while held; tiers 1/2/3; 85% group pick; lock raises to 100 and drops on reset/100; near-reset wait and no-row no-wait; stale active -> noActiveAccount; candidate filters). RED, GREEN, commit.

### Task 5: Switch Log store (core)

**Files:** Create `apps/linux/core/src/auto_switch_log.rs`.

**Produces:** `Outcome { Switched = 0, Failed = 1, NoCandidate = 2 }` with `label()`, `Entry { id, date_unix, outcome, from_name, to_name, reason, detail }`, `AutoSwitchLogStore::{open, open_in_memory, insert(...) -> i64, fetch(limit), clear}`, max 500, macOS schema `auto_switch_logs(id, date, outcome, from_name, to_name, reason, detail)`.

- [ ] Tests: insert/fetch newest first, prune to max, clear, unknown outcome skipped. RED, GREEN, commit.

### Task 6: Controller (app)

**Files:** Create `apps/windows/app/src/auto_switch.rs`.

**Produces:** `Event { kind, account_name, date_unix }` with `text()`, `is_failure()`; `Controller::new(now, pick)`, `evaluate(&mut self, settings: &mut Settings, accounts: &[Account], log: &mut dyn FnMut(LogRow), perform: impl FnMut(&str) -> Result<(), SwitchError>) -> Option<String>`, `note_manual_switch(id, usage)`, `should_fast_poll(settings, accounts)`, `last_event()`; `controller()` global; `accounts_from(rows, availability, critical)`; footer helpers `shows_toggle(settings, rows)`, `near_limit(settings, rows)`.

- [ ] Tests: cooldown 600 s on target error, pause on active-login error, no-candidate logged once then re-armed, lock set after switch onto over-threshold target, critical switch holds, threshold switch clears hold, manual switch hold, priority dropped when account gone, baseline moved while home active, event and reason texts. RED, GREEN, commit.

### Task 7: Refresh-loop wiring (app)

**Files:** Modify `apps/windows/app/src/main.rs`, `refresh.rs`, `cc_state.rs`.

**Produces:** `cc_state::BUSY` + `try_begin()/end()`; `refresh::FastPoll { due }` with `update(on, now)`, `remaining(now)`, `fired(now)`; loop runs `auto_switch::run(&mut out, now)` after `cc_state::refresh`, relabels after a switch; manual switch calls `note_manual_switch`; `Nudge::Local` re-evaluates.

- [ ] Tests: FastPoll arm/disarm/fire; busy flag exclusive. RED, GREEN, commit.

### Task 8: Cards: bolt, flames, account-page toggle (UI)

**Files:** `apps/windows/app/ui/components.slint`, `app.slint`, `src/model.rs`, `main.rs`.

**Produces:** `UiRow` fields `is-home`, `can-tick-home`, `critical-level` (0/1/2), `critical-help`; callbacks `toggle-home(string)`. Bolt tooltips as macOS.

- [ ] Tests: `model` fills the fields (home, can tick only ready/active with usage, critical level/help). Commit.

### Task 9: Flyout footer (UI)

**Files:** `app.slint` PopoverWindow, `main.rs`.

**Produces:** properties `auto-switch-shown`, `auto-switch-on`, `auto-switch-near`, `auto-switch-event`, `auto-switch-event-failed`; callback `set-auto-switch(bool)`.

- [ ] Tests: footer fields helper (shown when on, or near and widget toggle; event text with time). Commit.

### Task 10: Switch Log pane (UI)

**Files:** Create `ui/switch_log.slint`, `src/switch_log_view.rs`; modify `app.slint` (Pane.switch-log, sidebar item after Ping Log, header Refresh / Clear All with confirm).

- [ ] Tests: row mapping (accounts "a → b", "Unknown account", outcome colors), date format. Commit.

### Task 11: X button and chart (UI)

**Files:** Create `src/ratio.rs`, `ui/ratio.slint`; modify `app.slint` header.

**Produces:** `ratio::label(x, default) -> String`, `ratio::help(x, updated_at, offset) -> String`, hourly thread, chart points model, range 7d/30d/All, hover nearest.

- [ ] Tests: label/help texts, nearest point, controller default fallback and retention. Commit.

### Task 12: Settings sections and tips (UI)

**Files:** `app.slint` GeneralSettingsPane, `src/settings_general.rs`, `src/tips.rs`, `main.rs`.

- [ ] Tests: tips table holds the 12 macOS tips with macOS texts; settings setters clamp and keep high pace >= pace; Reset All text. Commit.

### Task 13: Help, docs

**Files:** `src/help.rs`, `ui/help.slint`, `contract/windows.md`, `CLAUDE.md`.

- [ ] Tests: TOPICS order matches macOS. Commit.
