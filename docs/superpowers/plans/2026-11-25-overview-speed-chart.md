# Overview Speed Chart Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a separate Speed chart (Max-equivalent %/h per account plus a Total line) to the macOS Overview pane, switched with the existing Usage chart, on rules shared through `contract/` and implemented in Swift and Rust `core`.

**Architecture:** A pure bucket function and a pure Total function, specified by `contract/cases/speed-series.json`, exist twice: Rust `core::speed_series` and Swift `SpeedSeries`. Both are tested against that one fixture. The macOS Overview keeps one `InteractiveChartContainer` (time toolbar, range, presets, live edge) and swaps the chart inside it: the existing Usage chart body or a new `OverviewSpeedChart` view. Range, window and legend selection are therefore shared for free.

**Tech Stack:** Swift 5 / SwiftUI / Swift Charts (macOS 13), XCTest; Rust (`apps/linux/core`), serde_json, cargo test.

**Spec:** `docs/superpowers/specs/2026-11-25-overview-speed-chart-design.md`

## Global Constraints

- Steps: 6, 10, 20, 30 minutes; default 6. No smoothing.
- Every drawn value is Max-equivalent %/h: speed x `weight(plan) / 10`. Weights: Pro 1, Max 5x 5, Max 20x 20, Max 10, Team 1, Enterprise 1, Free 1.
- Values shown with one decimal (`12.5`), on axis ticks and in the tooltip.
- Only one chart on screen at a time; the switch "Usage | Speed" sits in the pane header.
- The two charts share the visible range, the 5h / 7d / Fable choice and the legend selection.
- The "Avg" pill is hidden in Speed; unchanged in Usage.
- The Usage chart's code and behaviour are unchanged.
- App code reads `AppDefaults.shared`, never `UserDefaults.standard`. Keys: `overviewChartMode` ("usage" | "speed"), `overviewSpeedStepMinutes` (6 | 10 | 20 | 30).
- No Windows or Linux UI in this plan.
- After adding Swift files: `cd apps/macos && xcodegen generate`.
- No Claude attribution in commit messages.

## The algorithm (shared by Task 1 and Task 2)

Inputs: entries `(t seconds, u percent)` for one account, sorted by `t`, after the reset-transition pass; step `S` seconds; `utcOffset` seconds; range `[from, to]`; `factor = weight/10`.

- Bucket boundaries are local-clock aligned: `start(t) = floor((t + utcOffset) / S) * S - utcOffset`.
- Candidate buckets are those that overlap the range: `end > from` and `start < to`.
- A bucket gets a point only if `firstEntry.t <= start` and `lastEntry.t >= end`.
- For each consecutive pair `(a, b)` with `b.u > a.u`:
  - If `b.t > a.t`, the increase is spread linearly over `[a.t, b.t]`, and every bucket gets `(b.u - a.u) * overlap / (b.t - a.t)`.
  - If `b.t == a.t`, the whole increase goes to the bucket that contains `b.t`.
- Pairs where `u` falls or stays the same contribute 0.
- `value = sum / (S / 3600) * factor`. Each point is stamped at its bucket **end**.

Total, given per-account points (already scaled) plus each account's last entry time and `now`:
- Sum the values per bucket end over the accounts that have a point at that end.
- An account is *live* if `now - lastEntry <= 900`.
- If any account is live, drop the Total points after `min(last point end)` over the live accounts that have at least one point.

## Review Focus

1. An account with no logs in the range, or one just added: it has no line, Total is built from the others, and nothing crashes. Test: Task 3 `testAccountWithoutLogsIsSkipped`.
2. Every account deselected in the legend: the Speed chart shows no lines and no Total, and the y axis stays 0 to 10. Test: Task 3 `testNoSelectionGivesNoLines` and `testYAxisMaxFloorsAtTen`.
3. A long, compressed plateau on the 7d or Fable window, where two equal rows are hours apart: it must read 0, not a gap and not a spike. Fixture case "compressed plateau" (Task 1).
4. App closed overnight, then a big rise: it is spread over the gap, not drawn as one spike. Fixture case "overnight gap" (Task 1).
5. A 30-day range at a 6-minute step (7 200 buckets x accounts) must stay responsive. Test: Task 3 `testThirtyDaysTenAccountsIsFast` (under 1 s).

---

### Task 1: Contract and Rust `core::speed_series`

**Files:**
- Create: `contract/cases/speed-series.json`
- Modify: `contract/README.md`: new section "## Speed series" after "## Chart range label". It states the algorithm above, the weight table and the factor, and that each fixture is the source of truth.
- Create: `apps/linux/core/src/speed_series.rs`
- Modify: `apps/linux/core/src/lib.rs`: add `pub mod speed_series;`
- Create: `apps/linux/core/tests/contract_speed_series.rs` (copy the fixture-path helper from `contract_chart_window_summary.rs`)
- Modify: `apps/windows/app/src/overview.rs:50-58`: delete the local `plan_weight` and replace it with `pub use claude_dashboard_core::speed_series::plan_weight;` so callers and the existing test `plan_weights_follow_macos` keep compiling.

**Interfaces:**
- Produces (Rust):
  - `pub fn plan_weight(plan: &str) -> f64` (keyed by label: "Pro", "Max 5x", "Max 20x", "Max", "Team", "Enterprise", "Free"; unknown -> 1.0)
  - `pub fn max_factor(plan: &str) -> f64` (= `plan_weight / 10.0`)
  - `pub struct SpeedPoint { pub end_ms: f64, pub value: f64 }`
  - `pub fn account_speed(entries: &[chart::Entry], step_ms: f64, utc_offset_ms: f64, from_ms: f64, to_ms: f64, factor: f64) -> Vec<SpeedPoint>`
  - `pub struct AccountSpeed<'a> { pub points: &'a [SpeedPoint], pub last_entry_ms: f64 }`
  - `pub fn total_speed(accounts: &[AccountSpeed], now_ms: f64) -> Vec<SpeedPoint>`
- Produces (fixture shape, consumed by Task 2): an object with three arrays.

- [ ] **Step 1: Write the fixture `contract/cases/speed-series.json`**

Times in the fixture are in seconds. `expect` holds `[end, value]` pairs, and tests compare values within 1e-9.

```json
{
  "factors": [
    {"plan": "Pro", "factor": 0.1}, {"plan": "Max 5x", "factor": 0.5},
    {"plan": "Max 20x", "factor": 2.0}, {"plan": "Max", "factor": 1.0},
    {"plan": "Team", "factor": 0.1}, {"plan": "Enterprise", "factor": 0.1},
    {"plan": "Free", "factor": 0.1}
  ],
  "series": [
    {"name": "one 1% step in a 6-min bucket reads 10.0", "step_min": 6, "utc_offset_s": 0, "from": 0, "to": 720, "plan": "Max",
     "points": [[0, 5], [120, 5], [240, 6], [720, 6]], "expect": [[360, 10.0], [720, 0.0]]},
    {"name": "Pro is scaled to a tenth", "step_min": 6, "utc_offset_s": 0, "from": 0, "to": 720, "plan": "Pro",
     "points": [[0, 5], [120, 5], [240, 6], [720, 6]], "expect": [[360, 1.0], [720, 0.0]]},
    {"name": "Max 20x is doubled", "step_min": 6, "utc_offset_s": 0, "from": 0, "to": 720, "plan": "Max 20x",
     "points": [[0, 5], [120, 5], [240, 6], [720, 6]], "expect": [[360, 20.0], [720, 0.0]]},
    {"name": "a segment across two buckets splits by time", "step_min": 6, "utc_offset_s": 0, "from": 0, "to": 720, "plan": "Max",
     "points": [[0, 0], [300, 0], [420, 2], [720, 2]], "expect": [[360, 10.0], [720, 10.0]]},
    {"name": "a reset drop counts 0, never negative", "step_min": 6, "utc_offset_s": 0, "from": 0, "to": 720, "plan": "Max",
     "points": [[0, 40], [180, 50], [181, 0], [360, 3], [720, 3]], "expect": [[360, 130.0], [720, 0.0]]},
    {"name": "compressed plateau over hours reads 0", "step_min": 30, "utc_offset_s": 0, "from": 0, "to": 7200, "plan": "Max",
     "points": [[0, 20], [7200, 20]], "expect": [[1800, 0.0], [3600, 0.0], [5400, 0.0], [7200, 0.0]]},
    {"name": "overnight gap is spread, no spike", "step_min": 30, "utc_offset_s": 0, "from": 0, "to": 3600, "plan": "Max",
     "points": [[0, 10], [36000, 40]], "expect": [[1800, 3.0], [3600, 3.0]]},
    {"name": "buckets follow the local clock at +5:30", "step_min": 20, "utc_offset_s": 19800, "from": 600, "to": 3000, "plan": "Max",
     "points": [[0, 0], [1800, 0], [1801, 1], [3600, 1]], "expect": [[1800, 0.0], [3000, 3.0]]},
    {"name": "a same-time rise goes to the bucket holding it", "step_min": 6, "utc_offset_s": 0, "from": 0, "to": 720, "plan": "Max",
     "points": [[0, 0], [100, 0], [100, 1], [720, 1]], "expect": [[360, 10.0], [720, 0.0]]},
    {"name": "partly covered edge buckets get no point", "step_min": 6, "utc_offset_s": 0, "from": 0, "to": 1080, "plan": "Max",
     "points": [[100, 0], [700, 6]], "expect": []},
    {"name": "10-min step", "step_min": 10, "utc_offset_s": 25200, "from": 0, "to": 1200, "plan": "Max",
     "points": [[0, 0], [300, 1], [1200, 1]], "expect": [[600, 6.0], [1200, 0.0]]}
  ],
  "total": [
    {"name": "sums the accounts per bucket", "now": 10000,
     "accounts": [{"points": [[360, 10], [720, 0]], "last_entry": 9990}, {"points": [[360, 1], [720, 2]], "last_entry": 9990}],
     "expect": [[360, 11.0], [720, 2.0]]},
    {"name": "an account missing a bucket adds nothing there", "now": 10000,
     "accounts": [{"points": [[360, 10], [720, 5], [1080, 5]], "last_entry": 9990}, {"points": [[720, 1], [1080, 1]], "last_entry": 9990}],
     "expect": [[360, 10.0], [720, 6.0], [1080, 6.0]]},
    {"name": "stops where a live account has not caught up", "now": 1100,
     "accounts": [{"points": [[360, 1], [720, 1], [1080, 1]], "last_entry": 1090}, {"points": [[360, 2], [720, 2]], "last_entry": 1000}],
     "expect": [[360, 3.0], [720, 3.0]]},
    {"name": "a stale account does not hold Total back", "now": 5000,
     "accounts": [{"points": [[360, 1], [720, 1], [1080, 1]], "last_entry": 4990}, {"points": [[360, 2], [720, 2]], "last_entry": 1000}],
     "expect": [[360, 3.0], [720, 3.0], [1080, 1.0]]},
    {"name": "no accounts, no Total", "now": 1000, "accounts": [], "expect": []}
  ]
}
```

(Check of the 10-minute case: buckets at +7h line up with UTC at 10-minute boundaries. The rise from 0 to 300 is 1% in bucket [0,600), and 1 / (1/6 h) = 6.0.)

- [ ] **Step 2: Write `contract_speed_series.rs`**

The test file has three tests: `speed_series_factors` (`max_factor(plan) == factor`), `speed_series_cases` and `speed_series_total_cases`. Convert seconds to ms on the way in and ms to seconds on the way out. Every message includes the case `name`.

- [ ] **Step 3: Run it and watch it fail**

Run: `cd apps/linux && cargo test -p claude-dashboard-core --test contract_speed_series`
Expected: a compile error, because `speed_series` does not exist yet.

- [ ] **Step 4: Implement `speed_series.rs`**

Implement the functions from the Interfaces block, following "The algorithm" above. Doc comments name the fixture and the README section.

- [ ] **Step 5: Run the core suite and watch it pass**

Run: `cd apps/linux && cargo test -p claude-dashboard-core`
Expected: every test passes, including the 3 new ones.

- [ ] **Step 6: Swap the Windows `plan_weight` for the re-export**

Replace the function with the re-export. Then run `cd apps/windows && cargo test -p claude-dashboard overview` if this machine can build the Windows app. If it cannot (it is a Windows-only Slint/Win32 crate), say so in the commit body: CI's Windows job runs `cargo test -p claude-dashboard`.

- [ ] **Step 7: Commit**

```bash
git add contract/cases/speed-series.json contract/README.md apps/linux/core/src/speed_series.rs apps/linux/core/src/lib.rs apps/linux/core/tests/contract_speed_series.rs apps/windows/app/src/overview.rs
git commit -m "feat(core): speed series rules in contract and core::speed_series"
```

### Task 2: Swift `SpeedSeries` and `PlanWeight`

**Files:**
- Create: `apps/macos/ClaudeDashboard/Models/SpeedSeries.swift`
- Create: `apps/macos/ClaudeDashboardTests/SpeedSeriesContractTests.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/OverviewChartView.swift`: delete `private static func planWeight`; `weightedAverageRate` calls `PlanWeight.weight(_:)` instead (same values, so the Avg number is unchanged).

**Interfaces:**
- Consumes: the fixture from Task 1.
- Produces (Swift):
  - `enum PlanWeight { static func weight(_ plan: AccountPlan) -> Double; static func maxFactor(_ plan: AccountPlan) -> Double }`
  - `enum SpeedStep: Int, CaseIterable, Identifiable { case six = 6, ten = 10, twenty = 20, thirty = 30 }`, with `var seconds: TimeInterval` and `var label: String` ("6m", "10m", "20m", "30m")
  - `struct SpeedPoint: Equatable { let end: Date; let value: Double }`
  - `enum SpeedSeries`:
    - `static func account(entries: [UsageLogEntry], step: TimeInterval, utcOffset: TimeInterval, range: ClosedRange<Date>, factor: Double) -> [SpeedPoint]`. Entries belong to one account, sorted by `recordedAt`.
    - `struct AccountInput { let points: [SpeedPoint]; let lastEntry: Date }`
    - `static func total(_ accounts: [AccountInput], now: Date) -> [SpeedPoint]`

- [ ] **Step 1: Write `SpeedSeriesContractTests`**

Model it on `ChartWindowSummaryContractTests`. It decodes the whole fixture through `ContractFixtures.data("cases/speed-series.json")` and contains three tests:
- `testFactors`: `PlanWeight.maxFactor(AccountPlan(rawValue: plan)!) == factor`.
- `testSeriesCases`: build `UsageLogEntry`s from `points`, call `SpeedSeries.account` with `step: step_min * 60` and `factor: PlanWeight.maxFactor(plan)`, and compare `end.timeIntervalSince1970` exactly and `value` within 1e-9.
- `testTotalCases`.

Also add `testWeightsMatchAverageRate`: the weights are Pro 1, Max 5x 5, Max 20x 20, Max 10, Team/Enterprise/Free 1.

- [ ] **Step 2: Run it and watch it fail**

Run: `cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/SpeedSeriesContractTests`
Expected: a build failure, because `SpeedSeries` and `PlanWeight` are not defined.

- [ ] **Step 3: Implement `SpeedSeries.swift` and move `planWeight`**

Implement both, following "The algorithm" (same as Rust). Then point `weightedAverageRate` at `PlanWeight.weight`.

- [ ] **Step 4: Run the full app suite and watch it pass**

Run: `xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests`
Expected: `** TEST SUCCEEDED **`.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Models/SpeedSeries.swift apps/macos/ClaudeDashboardTests/SpeedSeriesContractTests.swift apps/macos/ClaudeDashboard/Views/OverviewChartView.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): SpeedSeries and shared PlanWeight, driven by the speed-series contract"
```

### Task 3: Speed chart on the Overview pane

**Files:**
- Create: `apps/macos/ClaudeDashboard/Views/OverviewSpeedChart.swift`. It holds the chart view and a pure `OverviewSpeedLines` builder.
- Create: `apps/macos/ClaudeDashboardTests/OverviewSpeedLinesTests.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/OverviewChartView.swift`: header switch, the swap inside the one `InteractiveChartContainer`, the step picker in `toolbarExtra`, and `averageRateProvider` set to nil in Speed.
- Modify: `apps/macos/Shared/AppDefaults.swift`: `static let overviewChartModeKey = "overviewChartMode"` and `static let overviewSpeedStepKey = "overviewSpeedStepMinutes"`.
- Modify: `apps/macos/ClaudeDashboard/Views/HelpView.swift`: one Overview line on the Usage | Speed switch, the step, and the Max-equivalent unit.

**Interfaces:**
- Consumes: `SpeedSeries`, `SpeedStep`, `SpeedPoint` and `PlanWeight` from Task 2.
- Produces:
  - `struct OverviewSpeedLines: Equatable { struct AccountLine: Equatable { let id: UUID; let points: [SpeedPoint] }; let accounts: [AccountLine]; let total: [SpeedPoint]; let yMax: Double }`
  - `static func make(logs: [UsageLogEntry], plans: [UUID: AccountPlan], order: [UUID], selected: Set<UUID>, step: SpeedStep, range: ClosedRange<Date>, now: Date, utcOffset: TimeInterval) -> OverviewSpeedLines`
    - `order` is `viewModel.accountStates` order, which keeps the colors stable.
    - Accounts outside `selected` and accounts with no logs are skipped.
    - `yMax = max(10, ceil(maxValue * 1.1 / 5) * 5)` over all account and total values.
  - `struct OverviewSpeedChart: View`, with inputs `lines: OverviewSpeedLines`, `range: ClosedRange<Date>`, `step: SpeedStep`, `names: [UUID: String]`, `colors: [UUID: Color]`. Its hover state is its own.

- [ ] **Step 1: Write `OverviewSpeedLinesTests`**

- `testScalesAndTotals`: a Max account and a Pro account, each with one 1% rise in bucket [0,360) (as in fixture case 1), range 0...720, `now` = 10000, both last entries at 9990. Expect Max points `[(360, 10.0), (720, 0.0)]`, Pro points `[(360, 1.0), (720, 0.0)]`, total `[(360, 11.0), (720, 0.0)]`.
- `testAccountWithoutLogsIsSkipped`: a selected account with no logs gets no entry in `accounts`, and Total equals the other account's line.
- `testNoSelectionGivesNoLines`: `selected` is empty, so `accounts` and `total` are empty and `yMax == 10`.
- `testYAxisMaxFloorsAtTen` and `testYAxisMaxRoundsUp`: with a max value of 0, `yMax == 10`; with a max value of 23, `yMax == 30` (23 x 1.1 = 25.3, rounded up to the next multiple of 5).
- `testThirtyDaysTenAccountsIsFast`: 10 accounts, one entry every 2 min for 30 days, rising 1% every 10 min. `make` with step `.six` over the full range finishes in under 1.0 s, timed with `Date()`.

- [ ] **Step 2: Run it and watch it fail**

Run: `cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/OverviewSpeedLinesTests`
Expected: a build failure, because `OverviewSpeedLines` is not defined.

- [ ] **Step 3: Implement `OverviewSpeedLines.make`**

Group `logs` by account (they already include the reset transitions and the out-of-range buffer points from `fetchLogs`), sort each group, and call `SpeedSeries.account` with `PlanWeight.maxFactor`. Build Total with `SpeedSeries.total` (`lastEntry` = the group's last `recordedAt`).

- [ ] **Step 4: Run the tests and watch them pass**

Run the same command as Step 2. Expected: `** TEST SUCCEEDED **`.

- [ ] **Step 5: Implement `OverviewSpeedChart` and wire it into `OverviewChartView`**

- `@AppStorage(AppDefaults.overviewChartModeKey, store: AppDefaults.shared) var mode = "usage"` and `@AppStorage(AppDefaults.overviewSpeedStepKey, store: AppDefaults.shared) var stepMinutes = 6`. An unknown stored step falls back to `.six`.
- `PaneHeader` trailing: a segmented `Picker` with "Usage" and "Speed", next to the existing spinner.
- Keep the single `InteractiveChartContainer`. Its `chartContent` shows either `overviewChart(range:)` (unchanged) or `OverviewSpeedChart`, so the container's range and preset state survive the switch. `toolbarExtra` adds a segmented step picker after `UsageWindowPicker` only in Speed mode. `averageRateProvider` is `mode == "speed" ? nil : { ... }`. Check that the container hides the pill when the provider is nil; if it does not, add that guard in the container.
- `OverviewSpeedChart`:
  - Lines: one `LineMark` series per account, 1.5 pt, in the account's existing color.
  - Total: a "Total" series, 3 pt, `Color.primary`.
  - Axes: x domain is `range`, with the same x axis marks and noon rules as the Usage chart (share the helpers by making `axisMarkDates` / `axisLabel` / `noonMarkers` `static` in `OverviewChartView` rather than copying them). Y domain is `0...yMax`, with tick labels `String(format: "%.1f", v)`. The axis title reads "%/h (Max)".
  - No limit markers.
- Tooltip:
  - Header: the hovered bucket's span, `HH:mm–HH:mm`. The bucket is the one with `end - step < hover <= end`.
  - Rows: each account's value, then Total, `%.1f`. An account without a point there shows `--`.
  - It flips sides like the Usage tooltip.
- `utcOffset` = `TimeZone.current.secondsFromGMT(for: range.upperBound)`.
- Compute `OverviewSpeedLines` once per change of logs, selection, step or range (not per hover): keep it in `@State` and rebuild it in `.onChange` / after `loadLogs` completes.

- [ ] **Step 6: Add the Help line, build, run the full suite**

Run: `xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests`
Expected: `** TEST SUCCEEDED **`.

- [ ] **Step 7: Check it in the running app** (use the `run` skill; Debug builds share the real store, so only read)

Expected, each confirmed with a screenshot:
1. Usage is the default and looks exactly as before.
2. Switching to Speed shows the same time range, account lines and a bold Total.
3. Panning in Speed, then switching back, keeps the panned range.
4. The step picker (6m default) changes the line density.
5. The Avg pill is hidden in Speed and back in Usage.
6. Quitting and relaunching restores the mode and step.
7. Deselecting every account leaves an empty Speed chart with a 0–10 axis.

- [ ] **Step 8: Commit**

```bash
git add apps/macos/ClaudeDashboard/Views/OverviewSpeedChart.swift apps/macos/ClaudeDashboard/Views/OverviewChartView.swift apps/macos/ClaudeDashboardTests/OverviewSpeedLinesTests.swift apps/macos/Shared/AppDefaults.swift apps/macos/ClaudeDashboard/Views/HelpView.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): Overview Speed chart with step picker and Max-equivalent Total"
```

### Task 4: Docs

**Files:**
- Modify: `CLAUDE.md`, the Views list: one line saying the Overview has Usage and Speed charts (`OverviewSpeedChart`, rules in `contract/cases/speed-series.json`, Rust `core::speed_series`).

- [ ] **Step 1: Edit, run `bash scripts/test-no-codename.sh`, commit**

Expected: the script exits 0.

```bash
git add CLAUDE.md && git commit -m "docs: note the Overview Speed chart"
```
