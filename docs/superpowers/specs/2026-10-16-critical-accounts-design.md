# Critical accounts and the 5h-to-7d ratio X (macOS)

Date: 2026-10-16
Status: approved design, awaiting implementation plan
Scope: `apps/macos/ClaudeDashboard` only. Linux, Windows, the helper CLI, `Account` and
`contract/` are untouched (auto switch exists only on macOS).
Builds on: `2026-10-13-auto-switch-design.md`.

## Goal

Spend weekly quota that would otherwise expire unused. An account is **critical** when even a
steady pace of use cannot finish its 7d quota before the 7d reset. The dashboard marks those
accounts, and auto switch moves Claude Code to one as soon as its 5h window is free.

Success: a critical account is visibly marked, and with auto switch on, Claude Code moves to it
at its next free 5h window without a click, while the home account (bolt tick) keeps its place
unless the critical account is in real danger.

## Delivery

One spec, two mergeable parts, built in order:

- **Part A, ratio X:** hourly estimate, history, Dashboard value and chart.
- **Part B, critical:** settings, badges, auto switch rules, Help and info tips.

Part A is useful on its own; Part B reads X from it.

## Part A: the ratio X

### Definition

`X` = the share of the 7d quota (in %) that one full 5h window (100% of 5h) costs. One value
for all accounts. Measured on 2026-10-16 from the last 3 days of `usage_logs.db`: 16.3%
over 39 windows (per-account 15.6 to 17.1%).

### Estimate

`UsageRatioEstimator.estimate(fiveHour:sevenDay:) -> UsageRatioEstimate?` (pure):

1. Input: every account's 5h and 7d log rows with `t >= now - 3 days`.
2. Per account, walk the 5h rows in time order; a row whose `resetsAt` differs from the current
   window's by more than 60 s starts a new window (`resetsAt` jitters by a second or more between
   fetches, so grouping on the exact value splits windows).
3. For each window take its first and last row (`t0, u0`) and (`t1, u1`). Read the account's 7d
   value at `t0` and at `t1` as the last 7d row at or before that time plus 5 s (the 5h and 7d
   rows of one refresh can land a second apart). Log compression only drops repeated values,
   so the step reading is exact. 7d rows are read from `now - 3 days - 7 days`, so a reading
   exists even when the value has not changed for days.
4. Skip the window when either 7d reading is missing, when the two readings' 7d `resetsAt`
   differ by more than 60 s (the week reset inside it), or when `u1 - u0 < 10` (7d is reported in whole
   percent; small windows are noise).
5. `X = 100 * sum(d7) / sum(d5)` over the kept windows.
6. Return nil when `sum(d5) < 100` (not enough data).

The result carries `x`, `sumFiveHour`, `sumSevenDay`.

### Schedule and fallback

`UsageRatioController` (`@MainActor`) runs the estimate at launch and then every hour.
A non-nil result is written to the store and becomes `currentX`. A nil result writes nothing;
`currentX` keeps the latest stored value, or the default **16** (flagged `isDefault`) when the
store is empty. Each run also deletes history older than 90 days.

### Store

`UsageRatioStore` (actor), `ratio_logs.db` next to `usage_logs.db`:

```
ratio_logs(id INTEGER PRIMARY KEY, t INTEGER NOT NULL, x REAL NOT NULL,
           d5 REAL NOT NULL, d7 REAL NOT NULL)
```

API: `insert(_:at:)`, `latest()`, `series(from:to:)`, `deleteOlderThan(_:)`.
Reads go through the existing `UsageLogStore.allLogs(window:from:to:)`.

### UI

- **Dashboard `PaneHeader`**, left of Refresh: a plain button `X 16.1%`. Tooltip: "100% of a 5h
  window costs about 16.1% of the 7d quota. Measured over the last 3 days, updated hourly at
  HH:mm". With the default: `X 16% (default)` in secondary color.
- **Click** opens a popover anchored to the button: Swift Charts line of X over time, range
  picker 7 days / 30 days / All, hover shows the nearest point's value and time, "No history
  yet" when empty.

## Part B: critical accounts

### Settings

`CriticalSettings` (`AppDefaults.shared`, like `AutoSwitchSettings`; stored values outside their
range fall back to the default):

| Setting | Key | Default | Range |
|---|---|---|---|
| Critical pace | `criticalPace` | 50 | 10...100, step 5 |
| High critical pace | `criticalHighPace` | 80 | criticalPace...100, step 5 |
| Quiet hours start | `criticalQuietStart` | 0 | 0...23 (hour) |
| Quiet hours end | `criticalQuietEnd` | 7 | 0...23 (hour); start == end means no quiet hours |
| Time zone | `criticalTimeZone` | `Asia/Ho_Chi_Minh` | any `TimeZone.knownTimeZoneIdentifiers` |

Raising Critical pace above High critical pace raises High critical pace with it.

### Assessment

`CriticalPlanner.assess(usage:x:settings:now:) -> CriticalAssessment` (pure), where
`CriticalAssessment { level: .none | .critical | .high; waste: Double }`:

```
activeHours(quiet) = hours in [now, 7d resetsAt) minus the parts inside quiet hours
                     (in the settings time zone; a quiet range may cross midnight)
projected(pace, quiet) = 7d + activeHours(quiet) / 5 * pace / 100 * X
```

- `.high` when `projected(highPace, no quiet hours) <= 100`;
- else `.critical` when `projected(pace, settings quiet hours) <= 100`;
- else `.none`.
- `waste = 100 - projected` of the level reached.
- `.none` when the 7d window has not started (`resetsAt == nil`), or usage is missing or stale.

Example (X = 16, pace 50, so 8% of 7d per window): 7d at 55% with 25 active hours (5 windows)
left projects 55 + 40 = 95, so the account is critical with waste 5.

### Badges

On `AccountCard` (full and compact), next to the bolt: `flame` orange for `.critical`,
`flame.fill` red for `.high`. Tooltip with the numbers, e.g. "High critical: at 80% of 5h per
window around the clock, 7d reaches 88% by its reset (12% unused)". Badges show whether auto
switch is on or off. Sidebar, account page and card order are unchanged.

### Auto switch rules

`AutoSwitchAccount` gains `critical: CriticalAssessment`, filled by the view model on each
refresh. A **critical target** is a candidate (existing `isCandidate`) with `7d < 99` whose 5h
window is free: `5h resetsAt == nil` or `5h <= 2` (2 tolerates an Auto Ping's 1%).

`AutoSwitchPlanner.decide`, after the existing active-account guard:

1. **High critical.** The active account is not `.high`, and some critical target is `.high`:
   switch to the `.high` target with the largest `waste`. Reason `.critical(.high)`.
2. **Home.** The existing priority reset, skipped while the active account is `.high`.
3. **Critical.** The active account is `.none` and is not the home account, and some critical
   target is `.critical` or `.high`: switch to the one with the largest `waste`. Reason
   `.critical(.normal)`.
4. **Threshold.** Unchanged: trigger, tiers 1 to 3, lock.

So: high critical beats home; home beats critical; critical beats an ordinary account.
No ping-pong: once Claude Code uses the target its 5h window starts and it stops being a
target; it leaves only through the threshold rules.

Footer: "Switched to {name}: high critical account" (or "critical account").
Switch Log reason: "High critical account's 5h window is free, {waste}% of 7d would go unused"
(or "Critical account's ...").

### Info tips and Help

- New component `InfoTip` (`Views/Components/`): an `info.circle` that shows a short tooltip on
  hover and, on click, a popover with the full explanation, a worked numeric example and a
  "More in Help" link to the topic.
- An `InfoTip` on **every** setting in Settings > General: auto refresh and its interval, Auto
  Ping, auto switch on/off, both thresholds, the menu bar toggle, all Critical Accounts settings,
  and auto-update.
- New Settings section "Critical Accounts" in `Views/Settings/CriticalSettingsSection.swift`,
  below Auto Switch.
- Help: new topic **Critical Accounts** (what X is and how it is measured, both formulas, quiet
  hours, the priority order, three worked examples); **Auto Switch** gains the new order;
  **Reading a Card** gains the flame badges and the X button.
- All UI and Help text is English, like the rest of the app.

## Modules

| Module | Kind | Depends on |
|---|---|---|
| `Services/Critical/UsageRatioEstimator.swift` | pure | none |
| `Services/Critical/UsageRatioStore.swift` | actor, SQLite | SQLite3 |
| `Services/Critical/UsageRatioController.swift` | `@MainActor` | estimator, both stores, clock |
| `Services/Critical/CriticalPlanner.swift` | pure | none |
| `Models/CriticalSettings.swift` | settings | `UserDefaults` (injected) |
| `Views/Components/InfoTip.swift` | view | none |
| `Views/Settings/CriticalSettingsSection.swift` | view | `CriticalSettings` |
| `Views/UsageRatioChart.swift` | view | `UsageRatioStore` |

`DashboardViewModel.swift` is at 962 lines: it only owns the controller and settings and
passes `critical` into `AutoSwitchAccount` (about 10 lines). Anything larger moves out.

## Testing

- `UsageRatioEstimatorTests`: 7d reset inside a window, `d5 < 10`, not enough data, several
  accounts, compressed (sparse) 7d rows.
- `UsageRatioStoreTests`: temporary DB; insert, latest, series range, retention.
- `UsageRatioControllerTests`: injected clock; launch run, hourly run, default fallback.
- `CriticalPlannerTests`: quiet hours crossing midnight, a time zone other than the system one,
  a DST day, start == end, 7d not started, exactly 100.
- `AutoSwitchPlannerTests`, one per branch: high pulls from home; critical does not pull from
  home; home pulls from critical but not from high; critical does not pull from another
  critical; target at 5h 3% is not eligible; largest waste wins; no ping-pong across two
  evaluations.
- `CriticalSettingsTests`: defaults, out-of-range values, high pace kept >= pace.

## Out of scope

Per-account X, X on Linux/Windows or in `contract/`, critical-aware ordering inside the
threshold tiers, sidebar or account-page badges.
