# Rocket Burn Level Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new current-rate rule for burn levels, a sixth level (rocket 🚀) above cheetah, and a per-account 60 s refresh while an account's 5h window burns at cheetah or rocket, on macOS and Windows (Linux JS gets the levels only).

**Architecture:** `contract/` first (rules + shared case files), then the shared Rust core (`apps/linux/core`, used by Windows), Linux JS, the rocket sprite, macOS, Windows. The tracker keeps the last three measurements per (account, window); level resolution is a pure function of (rate %/min, sustained seconds, utilization); "needs fast refresh" is a pure predicate each platform drives its 60 s poll from.

**Tech Stack:** Swift 5 / SwiftUI / XCTest; Rust (core + Slint app); ESM JS under gjs; Blender 5.2.3 LTS (build time only) + Python/Pillow.

**Spec:** `docs/superpowers/specs/2026-10-29-rocket-burn-level-design.md`

## Global Constraints

- Rate unit inside the tracker is **%/min**, computed as `deltaUtil * 60 / deltaSeconds` (multiply before divide, so 10 points in 60 s is exactly 10.0). Projected seconds = `(100 - utilization) * 60 / ratePerMin`.
- Minimum gap **50 s**: a measurement with `recordedAt - C.recordedAt < 50` is not stored and returns the key's last result (also covers non-positive gaps). Reset-cycle (60 s tolerance) and decrease checks run **before** this and still reset history.
- Window for A: use A only when `C.recordedAt - A.recordedAt <= 300`.
- Rocket: `ratePerMin >= 10`, or `ratePerMin >= 5 && sustainedSeconds >= 180`. Checked before the ladder. Level 6, emoji `🚀`, asset id `rocket`.
- Ladder levels 1-5 and boundaries unchanged (`> 5h`, `> 3h`, `> 1.5h`, `> 0.5h`, strict).
- Rate 0 -> `nil` (no result).
- Streak: starts at B.recordedAt of the first measurement whose rate is `>= 5`; any rate `< 5` or history reset clears it; `sustained = C.recordedAt - streakStart`.
- Fast refresh: 5h level `>= 5`, account active, no fetch error, Auto Refresh on. Period 60 s, first poll 60 s after entering, a full Auto Refresh re-arms it. Independent of `showBurnAnimals`.
- App code reads `AppDefaults.shared`, never `UserDefaults.standard`.
- No emojis in code comments; asset file names never use emoji.
- Commit messages: no Claude/Anthropic attribution. Work on a branch `feat/rocket-burn-level`, not `main`.
- Between tasks, platforms not yet updated may fail the shared contract tests (e.g. Swift after Task 1). Each task verifies its own suites; Task 8 runs everything.

## Review Focus

1. A burst of refreshes seconds apart (manual Refresh right after a poll) must not show a rocket or start fast refresh: pinned by tracker case "measurement 49 s after C is skipped".
2. Usage going flat must stop the 60 s poll within two polls: tracker case "flat twice within 5 minutes gives nothing" plus Task 6/7 poll tests that an account at level 4 / nil leaves the fast set.
3. Auto Refresh off, or the account expired/errored while burning, must stop fast refresh: `fast-refresh.json` cases.
4. A 5h reset during a rocket must drop the animal at once, not carry the streak into the new window: tracker case "reset cycle change clears history and streak".
5. Turning `showBurnAnimals` off must not stop fast refresh: the predicate takes no display flag (Tasks 2, 5), and the Windows helper test in Task 7 asserts targets with animals hidden.

---

### Task 1: Contract

**Files:**
- Modify: `contract/README.md` (sections "Burn-rate levels", "Burn-rate projection"; add "Fast refresh")
- Create: `contract/cases/burn-rate-rocket.json`, `contract/cases/burn-rate-tracker.json`, `contract/cases/fast-refresh.json`
- Modify: `docs/superpowers/specs/2026-10-29-rocket-burn-level-design.md` (call 8)

**Interfaces:**
- Produces: the three case files below, read by Swift, Rust and JS tests in later tasks. `burn-rate-levels.json` is unchanged.

- [ ] **Step 1: Write `burn-rate-rocket.json`** (array; fields `name`, `rate_pct_per_min`, `sustained_seconds`, `utilization`, `expect_level`, `expect_animal`, `expect_projected_seconds`):

| rate | sustained | util | level | projected |
|---|---|---|---|---|
| 10 | 0 | 50 | 6 🚀 | 300 |
| 9.99 | 0 | 50 | 5 🐆 | 300.3003003003003 |
| 5 | 180 | 10 | 6 | 1080 |
| 5 | 179 | 10 | 5 | 1080 |
| 4.99 | 600 | 10 | 5 | 1082.1643286573146 |
| 10 | 0 | 1 | 6 | 594 |
| 2 | 0 | 40 | 5 | 1800 |
| 1 | 0 | 0 | 3 🐇 | 6000 |

Tests compare projected with a 1e-9 tolerance.

- [ ] **Step 2: Write `burn-rate-tracker.json`**: `{"cases": [{"name", "steps": [{"utilization", "resets_at", "at", "expect_level": int|null, "expect_projected_seconds"?: number}]}]}`. Account and window are fixed by the test (`"a"`, 5h). `resets_at` is `100000` unless stated. Steps are `(util, at) -> expected` (`-` = null):

1. first measurement gives nothing: `(10,0)->-`
2. two measurements use B->C: `(10,0)->-`, `(11,60)->4/5340`
3. max of A->C and B->C within 5 minutes: `(10,0)->-`, `(16,60)->5/840`, `(16,120)->5/1680`, `(17,180)->4/4980`
4. A..C exactly 300 s uses A: `(10,0)->-`, `(20,150)->5/1200`, `(20,300)->4/2400`
5. A..C 301 s ignores A: `(10,0)->-`, `(20,150)->5/1200`, `(20,301)->-`
6. measurement 49 s after C is skipped and returns the last result: `(10,0)->-`, `(11,60)->4/5340`, `(30,109)->4/5340`, `(12,120)->4/5280`
7. 50 s gap is kept: `(10,0)->-`, `(11,50)->4/4450`
8. instant rocket at 10 %/min: `(10,0)->-`, `(20,60)->6/480`
9. sustained 5 %/min becomes rocket after 3 minutes: `(0,0)->-`, `(5,60)->5/1140`, `(10,120)->5/1080`, `(15,180)->6/1020`
10. a slower refresh breaks the streak: case 9's steps, then `(17,240)->5/1422.857142857143`, `(22,300)->5/936`
11. reset cycle change clears history and streak: `(10,0)->-`, `(20,60)->6/480`, `(21,120, resets_at 100061)->-`
12. resets_at jitter within 60 s is the same cycle: `(10,0)->-`, `(11,60, resets_at 100060)->4/5340`
13. decrease clears history: `(10,0)->-`, `(20,60)->6/480`, `(5,120)->-`, `(6,180)->3/5640`
14. flat twice within 5 minutes gives nothing: `(10,0)->-`, `(16,60)->5/840`, `(16,120)->5/1680`, `(16,180)->-`
15. reaching 100% while rising stays cheetah: `(95,0)->-`, `(100,60)->5/0`

- [ ] **Step 3: Write `fast-refresh.json`** (array; `name`, `five_hour_level` int|null, `is_active`, `has_error`, `auto_refresh_enabled`, `expect`): (5,T,F,T)->true; (6,T,F,T)->true; (4,T,F,T)->false; (null,T,F,T)->false; (5,F,F,T)->false; (5,T,T,T)->false; (5,T,F,F)->false.

- [ ] **Step 3b: `animal-display.json`**: add asset case `{ "name": "rocket has a stable asset id", "emoji": "🚀", "expect_asset_id": "rocket" }`.

- [ ] **Step 4: Update `contract/README.md`.** "Burn-rate levels": add level 6 and the order (rocket, then ladder); point at `burn-rate-rocket.json`. "Burn-rate projection": replace rules 4-5 with the three-measurement history, the 50 s gap, the A window, the streak and rate 0 -> nil, keeping rules 1-3 and the unconditional log write; point at `burn-rate-tracker.json`. New "Fast refresh" section: the predicate, 60 s period, first poll after 60 s, full refresh re-arms, independent of `showBurnAnimals`, Linux has none; point at `fast-refresh.json`. Add the rocket to the `showBurnAnimals` paragraph near line 60.

- [ ] **Step 5: Update spec call 8**: the burn poll and the auto switch poll stay independent on macOS (the 50 s rule makes a near-double fetch harmless); Windows fires both from one wait and sends one `Nudge::Some` with the union.

- [ ] **Step 6: Validate JSON**: `for f in contract/cases/burn-rate-*.json contract/cases/fast-refresh.json; do python3 -I -m json.tool "$f" >/dev/null || echo BAD $f; done` prints nothing.

- [ ] **Step 7: Commit** `docs(contract): rocket burn level, three-sample rate, fast refresh` (force-add the spec: `git add -f docs/superpowers/specs/2026-10-29-rocket-burn-level-design.md docs/superpowers/plans/2026-10-29-rocket-burn-level.md`).

### Task 2: Shared Rust core

**Files:**
- Modify: `apps/linux/core/src/burn_rate.rs`, `apps/linux/core/src/animal_display.rs`
- Test: `apps/linux/core/tests/contract_burn.rs`, `apps/linux/core/tests/contract_animal_display.rs`, unit tests in `burn_rate.rs`

**Interfaces:**
- Produces:
  - `BurnRateResult::from_rate(rate_pct_per_min: f64, sustained_s: f64, utilization: f64) -> BurnRateResult` (`from_projected_time` stays for the ladder).
  - `BurnRateTracker::record(&mut self, account_id: &str, window: i64, utilization: f64, resets_at: Option<f64>, recorded_at: f64) -> Option<BurnRateResult>` (signature unchanged, new rules).
  - `pub fn needs_fast_refresh(five_hour_level: Option<u8>, is_active: bool, has_error: bool, auto_refresh_enabled: bool) -> bool` in `burn_rate.rs`.
  - `pub const ROCKET_LEVEL: u8 = 6;` and `animal_display` maps `"🚀" => Some("rocket")`.

- [ ] **Step 1: Failing tests.** In `contract_burn.rs` add `burn_rate_rocket_cases` (from_rate vs `burn-rate-rocket.json`), `burn_rate_tracker_cases` (feeds each case's steps to a fresh tracker with account `"a"`, window `0`; asserts level/None and projected within 1e-9) and `fast_refresh_cases`. In `contract_animal_display.rs`, the existing asset cases plus a 🚀 -> `rocket` assertion. Replace the unit tests in `burn_rate.rs` that pin the removed rule 5 behaviour (`unchanged_stale_gap_ge_300_drops_rate` and any carried-rate test) with tests for: skipped measurement keeps history (a later step computes from the old C), streak resets on history reset.
- [ ] **Step 2:** `cd apps/linux && cargo test -p claude-dashboard-core --test contract_burn --test contract_animal_display` fails (missing `from_rate`, `needs_fast_refresh`).
- [ ] **Step 3: Implement.** `HistoryEntry` becomes `{ samples: Vec<Measurement> /* <= 3, oldest first */, streak_start: Option<f64>, last: Option<BurnRateResult> }`. `record`, in order:

```
first measurement            -> samples=[m], streak=None, last=None; return None
!same_cycle || util < C.util -> same reset; return None
recorded_at - C.at < 50      -> return last.clone()
push m, drop oldest beyond 3; (B, C) = last two, A = third-last
rate = (C.u - B.u) * 60 / (C.at - B.at)
if A and C.at - A.at <= 300: rate = max(rate, (C.u - A.u) * 60 / (C.at - A.at))
streak_start = if rate >= 5 { streak_start.or(Some(B.at)) } else { None }
last = (rate > 0).then(|| from_rate(rate, C.at - streak_start.unwrap_or(C.at), C.u))
return last.clone()
```

`from_rate`: projected = `(100 - utilization) * 60 / rate`; rocket check, else `from_projected_time(projected)` with the rocket's `projected_time` also set to projected. Update the struct doc comment (it names "the five rules").
- [ ] **Step 4:** `cd apps/linux && cargo test -p claude-dashboard-core` passes; `cargo clippy -p claude-dashboard-core --all-targets -- -D warnings` clean.
- [ ] **Step 5: Commit** `feat(core): rocket level, three-sample burn rate, fast refresh predicate`.

### Task 3: Linux JS

**Files:**
- Modify: `apps/linux/lib/burnRate.js`, `apps/linux/lib/colors.js`, `apps/linux/gnome-extension/helpWindow.js`
- Test: `apps/linux/tests/burnRate.test.js`, `apps/linux/tests/colors.test.js`

**Interfaces:**
- Consumes: the case files from Task 1.
- Produces: `colors.js` `BURN_ANIMALS[6] = '🚀'` and `burnRateFromRate(ratePctPerMin, sustainedSeconds, utilization) -> {level, animal, projectedSeconds}`; `BurnRateTracker.record({accountId, window, utilization, resetsAtMs, recordedAtMs})` returns that object or `null`, same rules as Task 2 (times in ms, rules in seconds). Same-cycle check gains the 60 s tolerance (it is exact equality today; both null is the same cycle, one null is different).

- [ ] **Step 1:** Add tests driving `burn-rate-rocket.json` (colors.test.js) and `burn-rate-tracker.json` (burnRate.test.js, `resets_at`/`at` seconds -> ms); replace tests pinning the removed hold/carry rules.
- [ ] **Step 2:** `cd apps/linux && CLAUDE_DASHBOARD_REPO="$(git rev-parse --show-toplevel)" gjs -m tests/run.js` fails on the new tests.
- [ ] **Step 3:** Implement the same algorithm as Task 2 Step 3; check every `indicator.js` use of the tracker result still works (`animal` field kept). Help line: add "a rocket means 5 %/min held for 3 minutes, or 10 %/min".
- [ ] **Step 4:** the gjs command passes.
- [ ] **Step 5: Commit** `feat(linux): rocket burn level and three-sample rate`.

### Task 4: Rocket sprite

**Files:**
- Modify: `tools/burn-animal-animation/common.py` (`ANIMALS` + `"rocket"` last), `build_scene.py` (rocket scene), `README.md` (ids, motion row), `burn_animals.blend` (regenerated), `manifest.json`
- Create: `tools/burn-animal-animation/rocket.png`, `apps/macos/ClaudeDashboard/Resources/BurnAnimals/rocket.png`, `apps/windows/app/ui/assets/burn-animals/rocket.png`
- Modify: both app `manifest.json` copies

**Interfaces:**
- Produces: asset id `rocket`, sheet `rocket.png` under the existing sheet contract (README "Contract").

- [ ] **Step 1:** Add the rocket scene to `build_scene.py` following the existing per-animal pattern: side view facing right, in place; body with nose cone, window and fins; flame and exhaust flicker (scale/shape keyed per frame, period dividing 24), body vibrates 1-2 px, 2-3 speed lines trail left; frame 0 calm (short flame); outline + light rim like the others. Motion row: "rocket | 4 | flame flickers and stretches, body vibrates, speed lines stream back".
- [ ] **Step 2:** Regenerate and render with `B=~/Applications/Blender.app/Contents/MacOS/Blender` using the README "Build, render, validate" commands (Blender first line must equal `BLENDER_VERSION`).
- [ ] **Step 3:** Copy `rocket.png` and `manifest.json` into both app asset dirs; `python3 -I tools/burn-animal-animation/validate_assets.py` on the tool dir and both app copies passes.
- [ ] **Step 4:** `python3 -I tools/burn-animal-animation/preview.py` and inspect the rocket strips at 52 px and 68 px on light and dark (Read the PNGs). Silhouette readable at 52 px, nothing clipped outside the 112 px box.
- [ ] **Step 5: Commit** `feat(assets): rocket burn sprite`.

### Task 5: macOS model, tracker and assets

**Files:**
- Modify: `apps/macos/ClaudeDashboard/Models/UsageLogModels.swift`, `Services/BurnRateTracker.swift`, `Models/AnimalDisplayModels.swift`, `Services/BurnAnimalAssetCatalog.swift` (only if it lists ids)
- Test: `apps/macos/ClaudeDashboardTests/BurnRateContractTests.swift`, `BurnRateTrackerTests.swift`, `AnimalDisplayModelsTests.swift`, `BurnAnimalAssetCatalogTests.swift`

**Interfaces:**
- Produces:
  - `BurnRateResult.animals[6] = "🚀"`, `static let rocketLevel = 6`, `static func from(ratePerMinute: Double, sustainedSeconds: TimeInterval, utilization: Double) -> BurnRateResult`.
  - `static func needsFastRefresh(fiveHourLevel: Int?, isActive: Bool, hasError: Bool, autoRefreshEnabled: Bool) -> Bool` on `BurnRateResult`.
  - `BurnRateTracker.record(accountId:window:utilization:resetsAt:recordedAt:) async -> BurnRateResult?` unchanged signature, Task 2 rules; still logs every call first.
  - `AnimalDisplayModels.assetIDs` gains `"rocket"`; `assetID(for: "🚀") == "rocket"`.

- [ ] **Step 1:** Contract tests for rocket cases, tracker cases (a temp `UsageLogStore` as the existing tracker tests build it; `recordedAt = Date(timeIntervalSince1970: at)`, `resetsAt` likewise; window `.fiveHour`) and fast-refresh cases; 🚀 asset case. Remove tracker tests that pin the removed rule 5.
- [ ] **Step 2:** `xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/BurnRateContractTests -only-testing:ClaudeDashboardTests/BurnRateTrackerTests -only-testing:ClaudeDashboardTests/AnimalDisplayModelsTests` fails.
- [ ] **Step 3:** Implement per Task 2 Step 3 (`HistoryEntry { samples: [Measurement], streakStart: Date?, last: BurnRateResult? }`). `resetTolerance` stays 60; add `static let minGap: TimeInterval = 50`, `aWindow: TimeInterval = 300`.
- [ ] **Step 4:** Full `ClaudeDashboardTests` scheme passes (run in background, log to a file).
- [ ] **Step 5: Commit** `feat(macos): rocket level and three-sample burn rate`.

### Task 6: macOS fast refresh and texts

**Files:**
- Create: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel+BurnPoll.swift`
- Modify: `ViewModels/DashboardViewModel.swift` (stored `burnPollTask`, call site at the end of `refreshAll`), `Views/Settings/SettingsTips.swift`, `Views/HelpView.swift` (`burnRateLevels` gains level 6 with range "5%/min for 3 min, or 10%/min")
- Test: `apps/macos/ClaudeDashboardTests/BurnPollTests.swift`

**Interfaces:**
- Consumes: `BurnRateResult.needsFastRefresh` (Task 5).
- Produces: `func burnFastIDs() -> Set<UUID>` (from `accountStates`: `burnRates?.fiveHour?.level`, `account.status == .active`, `error != nil`, `autoRefreshEnabled`); `func updateBurnPoll(afterFullRefresh: Bool)`.

- [ ] **Step 1:** `BurnPollTests`: `burnFastIDs` returns only the cheetah/rocket active, error-free account; empty when `autoRefreshEnabled` is false; unaffected by `showBurnAnimals = false`. Build the view model the way existing view-model tests do (memory: stub `BrowserCookieService` too; never touch real defaults).
- [ ] **Step 2:** Run it; fails (no `burnFastIDs`).
- [ ] **Step 3:** Implement. `updateBurnPoll`: empty set -> cancel and nil the task; task running and `afterFullRefresh` -> cancel and restart (re-arm); none running -> start. The task loops: sleep 60 s, `ids = burnFastIDs()`, empty -> nil the task and return, else `await refreshAll(only: ids)`. `refreshAll` calls `updateBurnPoll(afterFullRefresh: ids == nil)` right after `runAutoSwitch()`. Modelled on `updateAutoSwitchPoll` in `DashboardViewModel+AutoSwitch.swift`. Cancel the task where `autoSwitchPollTask` is cancelled (if anywhere) and when Auto Refresh is turned off (`scheduleAutoRefresh` path).
- [ ] **Step 4:** Tip text: replace "cheetah under 30m." with "cheetah under 30m, rocket at 5%/min held for 3 minutes or 10%/min. An account at cheetah or rocket refreshes every minute." in `SettingsTips.swift`. Full test scheme passes; `xcodebuild ... -scheme ClaudeDashboard build` succeeds.
- [ ] **Step 5: Commit** `feat(macos): refresh a fast-burning account every minute`.

### Task 7: Windows

**Files:**
- Modify: `apps/linux/core/src/rows.rs` (fields), `apps/windows/app/src/refresh.rs`, `main.rs` (refresh loop ~420-560 and the row builder ~1248), `model.rs`, `animal_assets.rs`, `auto_switch.rs` (test fixture), `tips.rs`, the Help topic that lists the animals (find with `grep -rn "cheetah" apps/windows/app`), `ui/theme.slint` comment
- Test: `refresh.rs`, `model.rs`, `animal_assets.rs`, `tips.rs` unit tests

**Interfaces:**
- Consumes: `BurnRateResult`, `needs_fast_refresh`, `ROCKET_LEVEL` (Task 2).
- Produces: `DisplayRow.burn`, `burn_7d`, `burn_fable: Option<BurnRateResult>` replacing `burn_projected_seconds{,_7d,_fable}`; `pub fn burn_fast_ids(rows: &[DisplayRow], auto_refresh_enabled: bool) -> Vec<String>` in `refresh.rs`.

- [ ] **Step 1: Tests.** `refresh.rs`: `burn_fast_ids_picks_cheetah_and_rocket_only` (levels 4/5/6/None, an errored and an expired row, auto refresh off -> empty); `apply_projections` existing test updated to the new field and a rocket sequence. `model.rs`: a row with `burn = Some(from_rate(10.0, 0.0, 50.0))` shows `🚀`, and with animals hidden shows "" (fast ids still non-empty for it). `animal_assets.rs`: id list includes `rocket`, sheet loads. `tips.rs`: detail contains "rocket".
- [ ] **Step 2:** `cd apps/windows && cargo test --workspace` fails.
- [ ] **Step 3: Implement.** `animal_for(r: &Option<BurnRateResult>) -> &'static str` uses `r.animal`. Refresh loop: a second `refresh::FastPoll` (`burn_poll`) and `burn_targets: Vec<String>`; after each job `burn_targets = burn_fast_ids(&out.rows, settings.auto_refresh_enabled)`, `burn_poll.update(!burn_targets.is_empty(), now)`, `rearm` on `Nudge::All` like `poll`; the wait takes the min of tick, `poll`, `burn_poll`; on timeout, if the tick is not due, fire both polls and send one `Nudge::Some` with the union of `poll_target` and `burn_targets` (deduped), nothing if both empty. Tip text as Task 6 Step 4.
- [ ] **Step 4:** `cd apps/windows && cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`; `cd apps/linux && cargo test --workspace` (rows.rs change). All pass. (GUI run-check needs Windows; note it as not run if not on Windows.)
- [ ] **Step 5: Commit** `feat(windows): rocket level and per-account fast refresh`.

### Task 8: Docs and full verification

**Files:**
- Modify: `CLAUDE.md`, `AGENTS.md` (the "Burn animal animation" bullet: rocket level, three-sample rate, 60 s fast refresh, spec path)

- [ ] **Step 1:** Update both bullets identically.
- [ ] **Step 2:** Run every suite: `ClaudeDashboardTests`, `ClaudeDashboardHelperTests`, gjs, `cargo test -p claude-dashboard-core` (apps/linux), `cargo test --workspace` (apps/windows), `node --test` (apps/windows/extension), `validate_assets.py` on the three asset dirs. All pass; paste the summary lines.
- [ ] **Step 3:** macOS run-check (skill `run`): Debug build against an isolated `CLAUDE_DASHBOARD_DEFAULTS_SUITE` (never the real store, per memory), with a fixture or stubbed usage that rises 10 points per minute for one account: the 5h ring alternates to the rocket, only that account is fetched every ~60 s (log), and fetching stops after usage goes flat. Screenshot the rocket.
- [ ] **Step 4: Commit** `docs: rocket burn level`.
