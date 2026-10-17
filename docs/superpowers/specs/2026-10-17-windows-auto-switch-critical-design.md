# Windows: Auto Switch, Switch Log, Critical accounts and X

Date: 2026-10-17
Status: approved (user: "làm hết đi", "cứ làm y như bản macOS")
Scope: `apps/windows` and new pure modules in `apps/linux/core`. Parity projects 4 and 5 of
the Windows roadmap, done as one branch because the planner is one unit: the macOS auto switch
rules already include the critical rules.
Reference (the source of truth): macOS `Services/AutoSwitch/*`, `Services/Critical/*`,
`Models/AutoSwitchSettings.swift`, `Models/CriticalSettings.swift`, `Models/AutoSwitchLogModels.swift`,
`ViewModels/DashboardViewModel+AutoSwitch.swift`, `+Critical.swift`, `Views/PopoverFooter.swift`,
`Views/AutoSwitchLogView.swift`, `Views/UsageRatioChart.swift`, `Views/Settings/*`, `Views/HelpView.swift`,
and `docs/superpowers/specs/2026-10-16-critical-accounts-design.md`.

## Intent

The Windows app must behave exactly like the macOS app. Everything below is a port: same
rules, constants, defaults, texts, tooltips, Help content and log formats. Where Windows has to
differ (no SwiftUI popovers, no Keychain), the closest Windows equivalent is used and noted.

## What the user gets

1. **Auto Switch** (on by default, like macOS). After every refresh the app decides whether
   Claude Code should move to another account, and does it silently through the existing
   `Switcher::switch_to` (ready accounts only, never a browser, never a prompt).
2. **Home account (bolt)** on every card (main window and flyout) and a "Switch here after 5h
   reset" toggle on the account page. One account at most.
3. **Flyout footer**: the Auto switch toggle and its last event on the right half, shown while
   auto switch is on, or near a limit when "Show toggle in menu bar when near limit" is on;
   dimmed (40%) outside the toggle zone.
4. **Switch Log** (Tools, after Ping Log): every switch, failed attempt and no-candidate stretch,
   newest first, 500 kept, Refresh and Clear All (asks first). `auto_switch_logs.db`.
5. **X**: hourly estimate of the 7d cost of a full 5h window, `ratio_logs.db`, 90 days. A button
   `X 16.3%` (or `X 16% (default)` in secondary color) in the Dashboard header left of Refresh;
   clicking opens a chart overlay with 7 days / 30 days / All and hover readout.
6. **Critical accounts**: orange flame (critical) or red filled flame (high critical) on cards
   with the macOS tooltip text.
7. **Settings > General**: an Auto Switch section (enable, 5h threshold 50-99, 7d threshold
   50-100, menu bar toggle) and a Critical Accounts section (critical pace, high critical pace,
   quiet hours start/end, time zone), each row with its InfoTip; Reset All covers them (home kept).
8. **Help**: topics Auto Switch and Critical Accounts (between Switch Claude Code and Ping),
   Reading a Card gains bolt, flame and X rows, Switch Claude Code gains the bolt row,
   Finding your way mentions the toggle and Switch Log. Texts copied from macOS.

## Rules (ported verbatim)

All constants from macOS:
- Planner: tier 5h limit 91, tier 7d limit 99, score cap 98, min days left 0.25, group ratio 0.85,
  reset tolerance 60 s, 5h window 5 h, free 5h limit 2, near-reset window 5 min, margin 1.
- Warning zone: 5h >= T5 - 3 or 7d >= T7 - 1. Toggle zone: 5h >= T5 - 15 or 7d >= T7 - 3.
- Decision order: high critical (not held, active not high) > home (active not high, 7d < 99,
  priority due) > critical (not held, active not critical and not home) > thresholds (lock-adjusted
  trigger, near-reset wait, tiers 1-3 with random pick in the 85% group).
- Controller: in-memory lock, hold, 10-minute per-target failure cooldown, 10-minute global pause
  when the failure is about Claude Code's current login (`ActiveAccountNotInDashboard`,
  `ActiveAccountUnknown`), no-candidate logged once per stretch, priority baseline maintenance,
  manual Switch -> `note_manual_switch` (clears lock, holds the target).
- Event texts, reason texts and error descriptions exactly as macOS `AutoSwitchEvent.text`,
  `AutoSwitchController.evaluate`, `describe`. `notCaptured`/`loginExpired` ->
  "it needs a browser sign-in first"; the two active-login errors -> "Claude Code's current login
  is not a dashboard account"; others -> `SwitchError::log_name()`.
- Critical: `activeHours`, `projected`, `assess`, `badge` texts, `CriticalSettings` ranges
  (pace 10-100 step 5, high pace >= pace, hours 0-23, default zone Asia/Ho_Chi_Minh, defaults
  50/80/0/7). Assessment is `.none` for a row with an error or a non-active status.
- X: `UsageRatioEstimator` (3-day lookback, 7d read from 10 days back, window delta >= 10,
  total >= 100, sample tolerance 5 s, negative 7d skipped), default 16, launch + hourly, 90-day
  retention, a nil estimate stores nothing.
- 60 s fast poll of the active account while auto switch is on and it is in the warning zone.

## Windows design (my calls)

- **Pure logic in core** (testable, shared): `core::auto_switch` (planner types + `decide` +
  helpers), `core::auto_switch_log` (SQLite store, `auto_switch_logs.db` beside `ping_logs.db`),
  `core::critical` (planner; time zones via `chrono-tz`, the only new dependency, needed for
  IANA zones with DST), `core::usage_ratio` (estimator + `ratio_logs.db` store). Settings keys go
  in `core::settings` with the macOS key names and the same out-of-range fallbacks.
- **Controller in the app** (`app/src/auto_switch.rs`): a global `Mutex` like `ping::CONTROLLER`.
  It runs on the refresh thread right after `cc_state::refresh`, so the switch (which can block
  up to 30 s waiting for a token refresh) never touches the UI thread. After a switch the thread
  re-captures, relabels the rows (badge moves) and then publishes. A shared `AtomicBool` in
  `cc_state` keeps a manual Switch and an auto switch from running together (macOS
  `isSwitchingClaudeCode`).
- **Turning it on / ticking the bolt / Reset All** sends `Nudge::Local` (no fetch): the loop
  re-evaluates on the current rows, matching macOS `Task { await runAutoSwitch() }`.
- **Fast poll**: a pure `FastPoll` timer inside the refresh loop; the loop waits for the earlier
  of the Auto Refresh tick and the poll; a poll fetches only the active account (`Nudge::One`).
- **Near-reset log**: the active account's 5h `usage_logs` rows of the last hour (newest 20).
- **X** (`app/src/ratio.rs`): a thread that recomputes at launch and every hour from
  `UsageLogStore::series_all`; the chart is drawn with the existing chart helpers style (a
  simple line + hover point in Slint, in an overlay card anchored under the header button,
  closed by clicking outside or Esc).
- **InfoTip** already exists on Windows; add the 8 new tips (texts from macOS `SettingsTips`) and
  update the Auto Refresh tip to the macOS text that mentions auto switch.
- **Time zone picker**: a ComboBox of every `chrono_tz::TZ_VARIANTS` name, sorted (macOS lists
  every known identifier, not searchable).
- **Dev runs** (`CLAUDE_DASHBOARD_FAKE_ROWS=1`): no switcher, so auto switch never switches;
  logs and ratio use the normal stores (as Ping does). Fake rows get a critical and a high
  critical example so the flames show.

## Non-goals

No change to macOS, Linux or `contract/` rules (only `contract/windows.md` documents Windows).
No per-account X. No new Windows-only behaviour.

## Testing

TDD per module: planner (one test per decision branch, as macOS `AutoSwitchPlannerTests`),
controller (cooldown, pause, lock, hold, no-candidate once, priority baseline, event texts),
critical (quiet hours across midnight, other zone, DST day, start == end, 7d not started,
exactly 100, badge texts), estimator (reset inside window, small delta, not enough data, several
accounts, sparse 7d), stores (insert/fetch/prune/clear; ratio insert/latest/series/retention),
settings (defaults, out-of-range, high pace >= pace, reset keeps home), fast poll timer, UI
field helpers (footer visibility/dim, flame, X label/help).
