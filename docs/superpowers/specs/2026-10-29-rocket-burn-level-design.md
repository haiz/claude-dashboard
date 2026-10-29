# Rocket burn level and fast refresh

Date: 2026-10-29. Status: draft for review.

## Intent

The burn animals (snail .. cheetah) tell how fast an account is using its 5h window. Heavy
sessions can burn far faster than "cheetah" conveys, and a 5-minute Auto Refresh is too
coarse to watch such a burn: the window can go from 50% to 100% between two refreshes.
This adds a sixth level, the rocket, for extreme burn, and makes a fast-burning account
refresh every minute so its numbers (and auto switch decisions built on them) stay current.

## Goals

1. A new rule for the **current burn rate**, used by every level (not only the rocket).
2. A sixth level, **rocket** (level 6, emoji 🚀, asset id `rocket`), above cheetah.
3. A **per-account 1-minute refresh** while that account burns at cheetah or rocket level;
   back to the configured Auto Refresh interval when it drops below cheetah.
4. macOS and Windows, kept in step through `contract/`.

## Non-goals

- No change to the projected-time thresholds of levels 1-5 (5h / 3h / 1.5h / 0.5h).
- No new setting. The thresholds and the 1-minute period are constants.
- No change to burn-rate sorting (it uses `utilization / timeRemaining`, not the tracker).

## Anti-goals

- Fast refresh must never refresh the whole fleet every minute: only the fast accounts.
- Fast refresh must not depend on `showBurnAnimals`: that switch is display only.

## Definitions (decided by the user)

**Current rate.** Per account and window, keep the last three measurements A -> B -> C in
the current reset cycle (C newest). Rate between two measurements = utilization increase
(percentage points) / seconds between them.

- **Minimum gap 50 s** (decided by the user after review). The API reports whole percents
  and refreshes often land seconds apart (manual Refresh, Ping, Switch, reset triggers, two
  60 s polls on separate clocks), so +1 point in a few seconds would read as a rocket: 47 of
  the 52 pairs at >= 10 %/min in the local `usage_logs.db` were under 50 s apart. A
  measurement less than 50 s after the newest measurement kept in the history (C) is still
  logged to the usage log store and still shown, but is not added to A -> B -> C and
  produces no new rate: the call returns the result last computed for that key (or `nil`).
  This rule applies after the reset-cycle and decrease checks, which still reset the history.
- If A exists and `C.recordedAt - A.recordedAt <= 300 s`: current rate =
  `max(rate(A->C), rate(B->C))`.
- Otherwise (only B and C exist, or A..C spans more than 5 minutes): current rate =
  `rate(B->C)`.

**Rocket.** A window's result is level 6 when either holds:

- current rate >= 10 %/min (100% in 10 minutes), or
- current rate >= 5 %/min (100% in 20 minutes) **and** it has stayed >= 5 %/min for 3
  minutes or more.

Otherwise the level comes from projected time to 100%, `(100 - C.utilization) / rate`, with
today's ladder (levels 1-5, strictly-greater-than boundaries, unchanged).

**Fast refresh.** An account whose **5h** result is level 5 (cheetah) or 6 (rocket) is
refreshed on its own every 60 s. When a refresh leaves its 5h result below level 5 (or no
result), it leaves the fast set and only follows Auto Refresh again.

## Calls made by Claude (check these)

1. **The new rate rule replaces the tracker's old rules for unchanged utilization** (the
   "carried `lastRate`" and "stale after 300 s" branches). They existed to keep an animal
   between flat polls; the A->C term now does that naturally (a flat C still has a positive
   A->C rate while A is within 5 minutes). Rules kept: first measurement, reset-cycle change
   with 60 s tolerance, utilization decrease, non-positive time gap.
2. **A current rate of 0 gives no result** (`nil`, no animal), as a flat window does today.
   Utilization at or above 100 with a positive rate still gives level 5, unless the rocket
   rule fires first.
3. **"Stayed >= 5 %/min for 3 minutes"** is measured from the earlier measurement (B) of
   the first refresh in the current unbroken run whose current rate was >= 5 %/min, up to
   now (C). Any refresh whose current rate is < 5 %/min, or any history reset, breaks the
   run. With 1-minute polls this means roughly the third fast poll.
4. **Rocket is checked before the projected-time ladder**, so a rocket can appear at low
   utilization, where projected time is still above 30 minutes.
5. **Fast refresh keys off the 5h window only**, the window the animals are drawn on. A
   cheetah 7d or Fable result does not start it.
6. **Fast refresh requires Auto Refresh to be on.** Turning Auto Refresh off means "no
   background fetches"; fast refresh honours that. It also only runs for `.active`
   accounts with no fetch error.
7. **The first fast poll comes 60 s after the account enters the fast set** (as the existing
   auto switch poll does). A full Auto Refresh that fetched the account reschedules its next
   fast poll 60 s from then, so it is never fetched twice in a row.
8. **Coexistence with the auto switch poll** (macOS `updateAutoSwitchPoll`, Windows
   `FastPoll`): both stay. When both want the same account in the same minute it is fetched
   once (macOS: one `refreshAll(only:)` with the union; Windows: one `Nudge::Some`).
9. **Linux** (`apps/linux/lib/burnRate.js`, `colors.js`, `core::burn_rate`) gets the same
   rate rule and level 6 with emoji 🚀 so the shared contract tests stay green, since
   Windows runs on `apps/linux/core`. The GNOME extension gets no fast refresh and no
   sprite: it shows emoji today, and this keeps it that way.
10. **Rocket sprite**: a side-view rocket facing right, flying in place: flame and exhaust
    flicker, body vibrates slightly, a few speed lines trail. Same sheet contract as the
    other animals (768 x 512, 6 x 4 cells of 128 px, 24 frames at 12 fps, seamless loop,
    frame 0 calm, outline and dark-theme rim). Built in `build_scene.py`, rendered by
    `render.py` with the pinned Blender, copied to both apps, `validate_assets.py` extended.
11. **Texts**: the Settings tip and Help topic on both platforms gain "rocket at 5 %/min held
    for 3 minutes, or 10 %/min" and mention the 1-minute refresh. The GNOME help line is
    updated for the rocket.

## Contract changes (do first)

- `contract/README.md`, "Burn-rate levels": add level 6 and its two conditions, and state
  the order (rocket check, then the ladder).
- `contract/README.md`, "Burn-rate projection": rewrite the rules section for the
  three-measurement history and the new rate; document the rocket streak (call 3).
- `contract/cases/burn-rate-levels.json`: keep the projected-time cases; add rocket cases
  that give `rate_pct_per_min` and `sustained_seconds` (boundaries: exactly 10 %/min is
  rocket, 9.99 is not; 5 %/min with 180 s is rocket, with 179 s is not; 4.99 with 600 s is
  not).
- New `contract/cases/burn-rate-tracker.json`: sequences of `(utilization, resets_at, at)`
  with the expected level / projected time / nil after each, covering: A->C vs B->C max,
  A..C exactly 300 s (uses A) vs 301 s (does not), a measurement 49 s after C is skipped
  (returns the last result) vs 50 s is kept, flat C within 5 minutes still shows,
  flat twice gives nil, the rocket streak building and breaking, reset-cycle and decrease
  resets. Swift, Rust and JS tests read it.
- New `contract/cases/fast-refresh.json` (or a section in an existing case file): given an
  account's 5h level, status, error and Auto Refresh flag, is it in the fast set.
- `contract/cases/animal-display.json`: asset case 🚀 -> `rocket`; eligibility unchanged.

## Platform touch points (from a read of the code; the builder confirms)

- **macOS**: `Models/UsageLogModels.swift` (`BurnRateResult`: level 1-6, keep
  `projectedTime`, add the rate), `Services/BurnRateTracker.swift` (history of three),
  `Models/AnimalDisplayModels.swift` (asset ids), `Resources/BurnAnimals/` (sprite +
  manifest), `ViewModels/DashboardViewModel*.swift` (fast set, a 60 s task like
  `updateAutoSwitchPoll` calling `refreshAll(only:)`), `SettingsTips.swift`, `HelpView.swift`.
- **Windows / shared Rust**: `apps/linux/core/src/burn_rate.rs`, `animal_display.rs`,
  `rows.rs` (`DisplayRow` carries only `burn_projected_seconds`; it must also carry the
  level, since the rocket cannot be derived from projected time), `apps/windows/app/src/
  refresh.rs` (`apply_projections`, a per-account fast poll next to `FastPoll`),
  `model.rs` (`animal_for`), `animal_assets.rs`, `ui/assets/burn-animals/`, `tips.rs`,
  `help.rs`.
- **Linux JS**: `lib/burnRate.js`, `lib/colors.js`, `tests/`, `gnome-extension/helpWindow.js`.

## Verification

- Contract case files drive Swift, Rust and JS tests; each new rule has a test that fails
  before the change.
- macOS: `ClaudeDashboardTests` green. Windows: `cargo test` in `apps/linux` (core) and
  `apps/windows`. Linux: `gjs -m tests/run.js`.
- `validate_assets.py` on the tool dir and both app copies; preview strips of the rocket
  at 52 px and 68 px, light and dark, reviewed by eye.
- A run-check on macOS with an injected fast-rising account (or the Windows fake rows)
  showing the rocket and the per-account 60 s fetch, and the fetch stopping when usage
  flattens.

## Left to the builder

Type and field names, how the fast set is stored, the exact Blender modelling of the
rocket within the sheet contract, and test file layout.
