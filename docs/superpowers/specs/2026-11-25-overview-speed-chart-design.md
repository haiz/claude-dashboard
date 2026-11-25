# Overview speed chart: design

Date: 2026-11-25. Status: approved by the user 2026-11-25.

## Intent

The Overview pane shows utilization (%) over time. The user wants to see **when during the day
tokens are spent fast and when slowly**. They will look at a real-time timeline (today, yesterday,
a given stretch) and read off the busy and quiet periods. A toggle switches the same chart between
the current utilization view ("Usage") and a usage-speed view ("Speed").

Success: on the Speed view, the user can point at a stretch of the timeline and say "between 09:00
and 11:00 I burned fast, after lunch it was slow", for each account and for all accounts together.

## Goals

- **Two separate charts** on the Overview pane, the existing Usage chart and a new Speed chart,
  with a **Usage | Speed** switch. Only one chart is on screen at a time (user: "2 charts, not
  1 chart with 2 modes").
- Speed view: one line per selected account, in %/h, plus one bold **Total** line.
- A step picker: **6 / 10 / 20 / 30 minutes**, default **6**. Each point is the speed over exactly
  one step (no smoothing). A jagged line is accepted by the user.
- Line chart (not bars).
- The same timeline as the Usage view: pan/zoom, range presets, live edge, the 5h / 7d / Fable
  window picker and the legend toggles all apply unchanged.
- The rules live in `contract/` and are implemented and tested in both Swift and Rust `core`.

## Non-goals

- A time-of-day profile averaged over many days (x axis 0h-24h). Considered and set aside by the
  user; may come later.
- Smoothing or moving averages.
- Windows UI in this project. Rust `core` gets the pure function and passes the contract fixture;
  the Windows Overview UI is a follow-up project.
- Linux (GNOME) UI.
- Any change to the existing Usage chart, its tooltip, or the "average rate" toolbar figure.

## Anti-goals

- Fake spikes: a big jump after the app was closed must not show up as one huge point.
- Negative speeds from resets.
- Summing raw percentages of different plans in the Total line as if they were the same unit.

## Constraints and facts (checked against the code and the real log, 2026-11-25)

- Polls land about every 2 minutes (median gap 123 s in `usage_logs.db`, last 24 h).
- `u` is always an integer percent in practice (129 367 five-hour rows, none fractional), so one
  step of 1% inside a 6-minute bucket reads as 10 %/h. The user accepted this quantization.
- The log store compresses plateaus (keeps first and last row of a run with the same
  `(aid, w, rat)` and value, `contract/usage-log.md`). A long gap between two rows with the same
  value is a real plateau (speed 0), not missing data. This happens mostly on the 7d and Fable
  windows.
- `withResetTransitions()` (Swift) / `with_reset_transitions` (Rust) already insert a synthetic
  drop to 0 at `resetsAt`. The speed math consumes entries after that pass.
- Plan weights already exist twice: Swift `OverviewChartView.planWeight` and Rust
  `apps/windows/app/src/overview.rs` `plan_weight`: Pro 1, Max 5x 5, Max 20x 20, Max (generic,
  `max200`) 10, Team / Enterprise / Free 1. The API cannot tell Max 5x from 20x, so consumer Max
  accounts are the generic Max (10). These values are estimates.
- `contract/` is the source of truth for cross-platform rules; Swift contract tests read
  `contract/cases/*.json` from the working tree.

## Approaches considered

1. **Compute inside the macOS view.** Fastest, but grows a 499-line view, is hard to test, and
   leaves Windows without the rules.
2. **Contract first, pure module in Swift and Rust, macOS UI now, Windows UI later.** *(chosen by
   the user)* The math is tested once against a shared fixture on both platforms; the UI work is
   split into two reviewable steps.
3. **As 2, plus the Windows Slint UI in the same project.** Ships both at once, roughly double the
   size, and the Windows UI can only be checked by running it.

The user picked approach 2.

## Decided by the user

- Timeline view (approach A), not a daily profile.
- Line chart.
- Step 6 / 10 / 20 / 30 minutes, default 6, no smoothing.
- **Every line is in Max-equivalent %/h** (user, after the builder check showed that raw account
  lines and a weighted Total cannot share one axis). Each account's speed is multiplied by
  `weight(plan) / weight(Max)` = `weight / 10`: Max x1, Pro / Team / Enterprise / Free x0.1,
  Max 5x x0.5, Max 20x x2. A Max account burning 20 %/h of its window shows 20.0; a Pro account
  burning 20 %/h shows 2.0.
- Total line = **plain sum of the visible account lines** (already in Max units), so Total is
  exactly what the lines add up to.
- Values are shown with **one decimal place** (axis labels, tooltip): `12.5`, not `12` or
  `12.50`.
- Architecture approach 2.

## The speed rule (to go into `contract/README.md`, "Speed series", and `contract/cases/speed-series.json`)

Input per account: entries `(t, u)` sorted by time, after the reset-transition pass; a step `S`
in seconds (360, 600, 1200, 1800); a range `[from, to]`.

1. **Buckets** are aligned to the **local clock**: `[k*S, (k+1)*S)` measured from the epoch shifted
   by the local UTC offset (an input to the pure function, like `noon_markers`' `local_offset_ms`),
   so buckets start at 09:00, 09:06, ... in any time zone, including +5:30 or +5:45. *(Builder
   check; settled by Claude.)*
2. **Utilization between two consecutive entries** is the straight line between them
   (linear interpolation). Only **increases** count: a segment where `u` falls (a reset or a
   synthetic drop) contributes 0.
3. **Speed of a bucket** = the sum of the increase of every segment's part that falls inside the
   bucket, divided by `S` in hours. Unit: %/h of that window.
   - Linear spreading is what keeps a jump after a long gap (app closed overnight) from becoming
     one giant point: the increase is spread over the whole gap.
   - A compressed plateau (two equal values hours apart) contributes 0 across the gap, which is
     correct.
4. **Which buckets get a point:** only buckets fully covered by the account's data, i.e. the
   first entry is at or before the bucket start and the last entry is at or after the bucket end.
   Buckets before the first entry, after the last entry, and the bucket still in progress at "now"
   get no point. (Builder's call to confirm the live-edge feel: the newest point lags by up to one
   step.)
   **Live edge for Total** *(builder check; settled by Claude)*: accounts are not polled in
   lockstep, so the newest bucket may have a point for some accounts and not yet for others. Total
   is drawn only up to the latest bucket that every *live* selected account has covered (live = its
   last entry is within 15 minutes of now). Older buckets follow rule 7 as written. This avoids a
   false dip at the right edge that corrects itself a minute later.
5. **Point position:** each point is drawn at its bucket's **end** time ("speed over the step
   ending here").
6. **Max-equivalent speed** of an account's bucket = speed x `weight(plan) / 10` (the generic
   Max weight). This is the value drawn and shown.
7. **Total** for a bucket = the sum of the Max-equivalent speeds of the selected accounts that
   have a point in that bucket. No account has a point -> no Total point. Unit: "Max-equivalent
   %/h".

Fixture cases to cover at least: Max-equivalent scaling for each plan (Pro 20 %/h -> 2.0); a single 1% step in a 6-min bucket (10 %/h); a segment that
straddles two buckets (split proportionally); a reset drop (0, never negative); a compressed
plateau over hours (0); an overnight gap with a large rise (spread, no spike); bucket alignment
for each of the four steps; partial edge buckets (no point); Total with Pro + Max weights; Total
when one account has no data in a bucket.

## Modules

- **Swift:** a pure `SpeedSeries` (new file under `apps/macos/ClaudeDashboard/Models/`) with the
  bucket function and the Total function, and a single `PlanWeight` table that both the existing
  "average rate" and the Total line use (moved out of `OverviewChartView`). Contract test
  `SpeedSeriesContractTests.swift` reads the fixture.
- **Rust:** `apps/linux/core/src/speed_series.rs` (new; `chart.rs` is already 627 lines) with the
  same functions, tested against the same fixture. Move `plan_weight` into `core` so Windows uses
  the same table later; `apps/windows/app/src/overview.rs` imports it from there.
- Plan weights are added to the contract (README section + fixture rows), since they now feed a
  displayed number on two platforms.

## macOS UI

```
[Usage | Speed]  [5h | 7d | Fable]  [6m v]        < 1d >   avg ...
%/h
 30 |          /\
 20 |    /\   /  \          == Total
 10 | /\/  \_/    \__/\     -- acc1  -- acc2
    +-------------------------------
     08:00   10:00   12:00   14:00
```

- The switch sits in the pane header, outside either chart. The step picker goes in the existing `toolbarExtra` slot of
  `InteractiveChartContainer`, next to `UsageWindowPicker`. The step picker shows only in Speed.
- Speed view: same accounts, same colors as Usage; the Total line is thicker, in the primary
  label color. Y axis: 0 to a rounded-up maximum of the visible points (at least 10), labelled
  "%/h (Max)", tick labels with one decimal. All lines share this one axis. No limit markers (the red warning points) in Speed.
- The "Avg x %/h" pill (plan-weighted mean) is **hidden in Speed view**; in Usage view it is
  unchanged. Showing a weighted mean next to a weighted-sum Total would read as two contradictory
  numbers. *(Builder check; settled by Claude.)*
- Hover tooltip in Speed: the bucket's time span, each account's Max-equivalent %/h and Total,
  one decimal each.
- Choice of view and step is remembered across launches in `AppDefaults.shared` (keys
  `overviewChartMode`, `overviewSpeedStepMinutes`). Never `UserDefaults.standard`.
- Data loading is unchanged: the Speed view uses the same `logs` the Usage view loads.
- The Speed chart is **its own view** (`OverviewSpeedChart`, own file), not a mode inside the Usage
  chart's body. The Usage chart code stays as it is. `OverviewChartView` becomes the host: header,
  switch, the one chosen chart, legend.
- *(Confirmed by the user)* The two charts share the visible time range,
  the 5h / 7d / Fable choice and the legend selection, so switching shows the same stretch of time
  in the other chart.

## Left to the builder

- Exact names, file split of the SwiftUI views, and how the Total line's color reads in light and
  dark mode.
- Y axis tick spacing.
- Performance: buckets are computed off the main actor if a 7-day range at a 6-min step
  (1 680 buckets x accounts) turns out slow.
- Help pane text for the new switch.
