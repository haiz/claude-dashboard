# Burn-rate animal animation in the 5h gauge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the static burn-rate animal overlays with a synchronized, polished running animation shown only in the large 5-hour percentage ring when the setting is enabled and the account's 5-hour usage changed within the last 10 minutes.

**Architecture:** Put the phase, frame-selection, and 600-second activity rules in pure models with contract fixtures. macOS owns one observable display clock and renders cached PNG sprite sheets in a fixed center `ZStack`; Windows owns one UI phase source and renders the same sprite sheets with Slint image clipping. Existing burn-rate projection remains the source of the selected animal, while stale activity only controls whether the animated layer is eligible.

**Tech Stack:** Swift 5 / SwiftUI / AppKit, Rust core and Windows Slint UI, Blender headless rendering, transparent PNG sprite sheets, existing XCTest and Cargo test suites.

---

## File map and boundaries

### Shared contract and pure models

- Create `contract/cases/animal-display.json` with phase, transition, frame, and activity boundary cases.
- Modify `contract/README.md` to document the 5h-only animated display contract and the existing default-off setting.
- Create `apps/linux/core/src/animal_display.rs` for Rust phase/frame/activity logic shared by the Windows app.
- Modify `apps/linux/core/src/lib.rs` to export `animal_display`.
- Create `apps/macos/ClaudeDashboard/Models/AnimalDisplayModels.swift` for the Swift mirror of the contract.
- Define one stable asset id mapping (`snail`, `turtle`, `rabbit`, `horse`, `cheetah`) from the existing burn-rate emoji values; never use an emoji glyph as a file name.
- Create `apps/macos/ClaudeDashboardTests/AnimalDisplayModelsTests.swift` for fixture-driven Swift tests.
- Add Rust unit tests in `apps/linux/core/src/animal_display.rs` and contract fixture loading where the existing core tests load JSON.

### Blender assets

- Create `tools/burn-animal-animation/BLENDER_VERSION` with the exact Blender executable version used for final renders.
- Create `tools/burn-animal-animation/burn_animals.blend` with five rigs/actions and fixed scene settings.
- Create `tools/burn-animal-animation/render.py` for headless deterministic rendering and sprite-sheet assembly.
- Create `tools/burn-animal-animation/validate_assets.py` for dimensions, alpha, bounds, loop, and size checks.
- Create `tools/burn-animal-animation/README.md` with install, render, validation, and review instructions.
- Create `tools/burn-animal-animation/manifest.json` and copy the validated manifest to `apps/macos/ClaudeDashboard/Resources/BurnAnimals/manifest.json` and `apps/windows/app/ui/assets/burn-animals/manifest.json`.
- Create `apps/macos/ClaudeDashboard/Services/BurnAnimalAssetCatalog.swift` and `apps/windows/app/src/animal_assets.rs` to validate the manifest and expose the stable id-to-sheet mapping with percentage-only fallback.

### macOS runtime

- Create `apps/macos/ClaudeDashboard/Services/AnimalDisplayClock.swift` for one main-actor clock shared by all gauges.
- Modify `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift` to own activity state, feed successful 5h observations, and start/stop the shared clock.
- Create `apps/macos/ClaudeDashboard/Views/Components/AnimatedBurnAnimalView.swift` for cached sprite-sheet frame selection and the 500 ms opacity/scale transition.
- Create `apps/macos/ClaudeDashboard/Services/BurnAnimalAssetCatalog.swift` for manifest validation and cached sheet lookup.
- Modify `apps/macos/ClaudeDashboard/Views/UsageBar.swift` to render the fixed center number/animal layers and remove the corner emoji overlay from the 5h path.
- Modify `apps/macos/ClaudeDashboard/Views/Components/UsageGaugeRow.swift` so only the 5h `UsageBar` receives animation data; 7d and Fable receive no animal.
- Modify `apps/macos/ClaudeDashboard/Views/AccountCard.swift`, `apps/macos/ClaudeDashboard/Views/AccountPane.swift`, `apps/macos/ClaudeDashboard/Views/DashboardPane.swift`, and `apps/macos/ClaudeDashboard/Views/MenuBarPopover.swift` to pass the shared phase and per-account 5h eligibility.
- Modify `apps/macos/ClaudeDashboard/Views/OverviewChartView.swift` and `apps/macos/ClaudeDashboard/Views/HelpView.swift` so the old legend/sample animal displays cannot bypass the 5h-only rule.
- Modify `apps/macos/ClaudeDashboard/Views/Settings/SettingsTips.swift` if the setting description needs to say “5h gauge only.”

### Windows runtime

- Modify `apps/windows/app/ui/theme.slint` to add the animation fields needed by `UiWindow` and retain the existing 7d/Fable fields.
- Modify `apps/windows/app/src/model.rs` to map the shared activity result into the 5h window only.
- Modify `apps/windows/app/src/refresh.rs` to record successful 5h observations without changing burn projection.
- Create `apps/windows/app/src/animal_assets.rs` for manifest validation and stable id-to-sheet mapping.
- Modify `apps/windows/app/ui/components.slint` to replace the 5h corner text with a centered clipped sprite and synchronized cross-fade; keep other windows unchanged.
- Modify `apps/windows/app/ui/app.slint` and `apps/windows/app/src/main.rs` to expose one app-wide phase/frame source to cards, the account page, and flyout.
- Modify `apps/windows/app/src/overview.rs` and `apps/windows/app/ui/chart.slint` so the old overview legend animal column is never used for this feature.
- Modify `apps/windows/app/ui/help.slint` and `apps/windows/app/src/tips.rs` to keep Help static and clarify the 5h-only setting.

## Task 1: Add and verify the cross-platform display contract

**Files:**
- Create: `contract/cases/animal-display.json`
- Modify: `contract/README.md`
- Create: `apps/linux/core/src/animal_display.rs`
- Modify: `apps/linux/core/src/lib.rs`
- Create: `apps/macos/ClaudeDashboard/Models/AnimalDisplayModels.swift`
- Test: `apps/macos/ClaudeDashboardTests/AnimalDisplayModelsTests.swift`

- [ ] **Step 1: Write the JSON fixture cases.**

  Include phase values at `0`, `5.5`, `6`, `9.5`, and just below `10`; expected
  content, transition progress, frame index, and stable animal asset id. Include activity cases for the
  first sample, utilization change, reset-cycle change within 60 seconds, reset
  cycle beyond 60 seconds, unchanged observations at `599.999` and `600`, just
  beyond `600`, failed refresh, and independent account ids.

- [ ] **Step 2: Add the Rust pure model and tests.**

  Implement constants for `DISPLAY_PERIOD_S = 10`, `NUMBER_TO_ANIMAL_S = 6`,
  `TRANSITION_S = 0.5`, `ANIMAL_FPS = 12`, `ANIMAL_FRAME_COUNT = 24`, and
  `ACTIVITY_TIMEOUT_S = 600`. Implement phase classification, exact cross-fade
  opacity, the frame formula from the spec, reset-tolerance comparison, and an
  account-keyed activity tracker. Keep all functions deterministic and free of
  clocks or UI types.

- [ ] **Step 3: Add the Swift mirror and fixture-driven tests.**

  Mirror the same constants and formulas in `AnimalDisplayModels.swift`; load
  the shared JSON fixture using the existing contract-test resource helper. Test
  every case before wiring views or refresh code.

- [ ] **Step 4: Run the focused RED/GREEN checks.**

  Run `cd apps/linux && cargo test -p claude-dashboard-core animal_display` and
  `xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/AnimalDisplayModelsTests`.
  Expected after implementation: all phase, frame, opacity, and activity cases pass.

- [ ] **Step 5: Commit the pure contract.**

  ```bash
  git add contract/cases/animal-display.json contract/README.md apps/linux/core/src/animal_display.rs apps/linux/core/src/lib.rs apps/macos/ClaudeDashboard/Models/AnimalDisplayModels.swift apps/macos/ClaudeDashboardTests/AnimalDisplayModelsTests.swift
  git commit -m "feat: define synchronized burn animal display"
  ```

## Task 2: Build reproducible Blender sprite sheets

**Files:**
- Create: `tools/burn-animal-animation/BLENDER_VERSION`
- Create: `tools/burn-animal-animation/burn_animals.blend`
- Create: `tools/burn-animal-animation/render.py`
- Create: `tools/burn-animal-animation/validate_assets.py`
- Create: `tools/burn-animal-animation/README.md`
- Create: `tools/burn-animal-animation/manifest.json`
- Create: `apps/macos/ClaudeDashboard/Resources/BurnAnimals/*.png`
- Create: `apps/macos/ClaudeDashboard/Resources/BurnAnimals/manifest.json`
- Create: `apps/windows/app/ui/assets/burn-animals/*.png`
- Create: `apps/windows/app/ui/assets/burn-animals/manifest.json`

- [ ] **Step 1: Install or select Blender and pin the exact version.**

  Install Blender only as a build-time asset tool, record `blender --version`
  in `BLENDER_VERSION`, and make the render script fail fast when the executable
  version differs. Do not add Blender or Photocraft to app dependencies.

- [ ] **Step 2: Create the five locomotion actions.**

  Build a consistent orthographic scene for snail glide, turtle trot, rabbit hop,
  horse gallop, and cheetah sprint. Use transparent film, fixed lighting, a
  restrained palette, and enough contrast for both light and dark app themes.
  Author a two-second 24-frame loop and review it at 52 px and 68 px before export.

- [ ] **Step 3: Render and assemble deterministic sheets.**

  Render RGBA frames into a 6-by-4 grid of 128-by-128 px cells, producing one
  768-by-512 px PNG sheet per animal and `tools/burn-animal-animation/manifest.json`
  with frame rate, Blender version, dimensions, source revision, stable asset id,
  and relative sheet path. Copy that exact manifest and sheets into both app
  resource locations during the asset checkpoint.

- [ ] **Step 4: Add measurable asset validation.**

  Validate 24 frames, exact dimensions, alpha channel, no opaque background,
  subject bounds within 112-by-112 px and within 8 px of cell center, first/last
  alpha-area difference under 10%, subject-center shift under 4 px, and a maximum
  2 MiB per sheet. Fail the command with the animal/frame that violates a rule.

- [ ] **Step 5: Review the animation visually and commit the asset checkpoint.**

  Run the render and validation commands from `tools/burn-animal-animation/README.md`,
  inspect the sheets at native gauge sizes, run both platform manifest validators,
  then commit the `.blend`, scripts, manifest, and generated sheets together.

## Task 3: Add macOS activity state and one shared display clock

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/AnimalDisplayClock.swift`
- Modify: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift`
- Test: `apps/macos/ClaudeDashboardTests/DashboardViewModelTests.swift`

- [ ] **Step 1: Write ViewModel tests for activity wiring.**

  Add tests proving a successful 5h utilization change makes the account eligible,
  unchanged refreshes do not move `lastChangedAt`, failed refreshes do not move it,
  `showBurnAnimals` does not affect burn-rate sorting, and the setting default/reset
  remains false.

- [ ] **Step 2: Implement the clock as one main-actor publisher.**

  Add a single timer-driven `AnimalDisplayClock` with monotonic elapsed phase and
  a 12 fps frame cadence. It starts only when the setting is enabled and at least
  one account is eligible, stops when no eligible account remains, and exposes
  one phase to all views. It must not create one timer per card.

- [ ] **Step 3: Feed only successful 5h observations.**

  In `DashboardViewModel.refreshAll`, after a successful usage result, send the
  account's 5h utilization and `resetsAt` to `AnimalActivityTracker`. Do not call
  it for failed/expired responses or 7d/Fable values. Keep the existing
  `BurnRateTracker.record` calls and sorting unchanged.

- [ ] **Step 4: Run the ViewModel tests and commit.**

  Run the focused DashboardViewModel tests and commit the clock/activity wiring
  before changing the view tree.

## Task 4: Render the animated animal in macOS's 5h ring

**Files:**
- Create: `apps/macos/ClaudeDashboard/Views/Components/AnimatedBurnAnimalView.swift`
- Create: `apps/macos/ClaudeDashboard/Services/BurnAnimalAssetCatalog.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/UsageBar.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/Components/UsageGaugeRow.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/AccountCard.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/AccountPane.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/DashboardPane.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/MenuBarPopover.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/OverviewChartView.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/HelpView.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/Settings/SettingsTips.swift`
- Modify: `apps/macos/project.yml` only if XcodeGen needs an explicit resource entry

- [ ] **Step 1: Add the sprite-sheet cache and frame view.**

  Validate the bundled manifest and stable id-to-file mapping once, load each
  PNG once from the app bundle, crop the selected 128-by-128 cell into a cached
  `CGImage`, and expose a SwiftUI `Image` without decoding on every refresh.
  Missing or malformed manifest/assets return nil so the caller keeps the
  percentage layer.

- [ ] **Step 2: Add fixed center layers to `CircularProgressView`.**

  Keep the ring and hover/click behaviour intact. Replace the bottom-right emoji
  overlay with a fixed-size center `ZStack`: percentage layer and animal layer
  share the same alignment box, use the exact contract opacities, and animate
  scale/opacity over 0.5 seconds. Preserve the percentage as the accessibility
  value. Only receive the animal payload when this `UsageBar` is the 5h gauge.

- [ ] **Step 2a: Thread the shared payload through every macOS caller.**

  Update `DashboardPane`, `MenuBarPopover`, and `AccountPane` to pass the one
  clock phase plus per-account 5h eligibility into `AccountCard`/`UsageGaugeRow`.
  Keep the explicit input boundary so a card cannot create a private clock or
  infer eligibility from 7d/Fable state.

- [ ] **Step 3: Remove legacy non-5h displays.**

  Make `UsageGaugeRow` pass no animal to 7d/Fable. Remove the animal from the
  Overview legend and Help sample. Update the settings tip to say the animated
  animal appears in the 5h percentage ring only.

- [ ] **Step 4: Generate and inspect Xcode resources.**

  Run `cd apps/macos && xcodegen generate`, build the app, and verify the generated
  bundle contains all five sheets. Do not rely on source-tree presence alone.

- [ ] **Step 5: Run macOS UI-focused tests and commit.**

  Run the existing gauge/layout tests plus the new display model tests. Commit the
  macOS animation implementation after confirming compact/full rendering.

## Task 5: Port activity and phase data into the Windows model

**Files:**
- Modify: `apps/windows/app/src/refresh.rs`
- Modify: `apps/windows/app/src/model.rs`
- Modify: `apps/windows/app/ui/theme.slint`
- Create: `apps/windows/app/src/animal_assets.rs`
- Test: `apps/windows/app/src/model.rs`
- Test: `apps/windows/app/src/refresh.rs`

- [ ] **Step 1: Write Windows model tests first.**

  Add cases proving only `five_hour` can carry animation eligibility, setting off
  clears it, unchanged/failed observations expire at 600 seconds, and 7d/Fable
  remain unchanged. Keep the existing `set_animals` retime behaviour intact.

- [ ] **Step 2: Add the shared activity fields to `UiWindow`.**

  Add the minimum fields needed by Slint: 5h eligibility, selected animal, and
  any stable display metadata. Do not put the global phase in every row; keep
  phase as an app-wide property so row refreshes do not restart animation.

- [ ] **Step 2a: Add stable asset ids and manifest validation.**

  Map the existing burn-rate animal values to the five stable ids, validate the
  packaged manifest at startup, and expose a boolean that makes the
  percentage-only fallback deterministic when a sheet or manifest entry is
  missing. Reference `ui/assets/burn-animals/<id>.png` with compile-time
  `@image-url` literals so the Windows build includes every sheet.

- [ ] **Step 3: Feed successful 5h samples from refresh.**

  Update the refresh path to record activity only after a successful active-account
  usage fetch. Keep projection and row sorting unchanged, and backfill errored rows
  without extending their activity timestamp.

- [ ] **Step 4: Run Windows/core tests and commit the model checkpoint.**

  On a Windows runner, run `cd apps/linux && cargo test -p claude-dashboard-core`
  for the shared core, then `cd apps/windows && cargo test -p claude-dashboard`
  for the Windows app model/refresh tests. On macOS, run only the
  platform-neutral core tests that compile there and record the Windows GUI
  limitation.

## Task 6: Render the synchronized animation in Windows Slint

**Files:**
- Modify: `apps/windows/app/ui/components.slint`
- Modify: `apps/windows/app/ui/app.slint`
- Modify: `apps/windows/app/src/main.rs`
- Modify: `apps/windows/app/src/animal_assets.rs`
- Modify: `apps/windows/app/src/overview.rs`
- Modify: `apps/windows/app/ui/chart.slint`
- Modify: `apps/windows/app/ui/help.slint`
- Modify: `apps/windows/app/src/tips.rs`

- [ ] **Step 1: Add one app-wide phase source.**

  Add an app property for the shared phase/frame and one UI-thread timer or
  monotonic animation tick. Start it only when the setting and at least one
  eligible 5h gauge require it. Pass the property through cards, account page,
  and flyout without creating nested timers.

- [ ] **Step 2: Replace the 5h corner emoji with a centered clipped sprite.**

  In `WindowColumn`, render the sprite sheet only when `label == "5h"` and the
  window is eligible. Use Slint image source clipping for the 6-by-4 sheet,
  exact frame index and opacity from the contract, and a fixed center box inside
  the existing `UsageGauge`. Keep the percent and ring layout unchanged.

- [ ] **Step 2a: Verify packaging and fallback.**

  Confirm the generated Slint resource bundle references all five compile-time
  sheet paths and that the startup manifest validator disables only the animal
  layer when metadata or a sheet is malformed. The ring, percent, and all other
  windows must continue rendering.

- [ ] **Step 3: Remove overview/help bypasses.**

  Make the overview legend hide its animal column for this feature and keep the
  Help sample static. Update the Windows setting tip/help text to describe the
  5h-only animation.

- [ ] **Step 4: Run the Windows visual check and commit.**

  Run `cd apps/windows && CLAUDE_DASHBOARD_FAKE_ROWS=1 cargo run -p claude-dashboard`, inspect
  dashboard/account/flyout at compact and full sizes, and verify all cards share
  phase. Commit after the Windows run-check is clean.

## Task 7: Cross-platform verification and documentation cleanup

**Files:**
- Modify: `contract/README.md` and `contract/windows.md` if wording is incomplete
- Modify: `tasks/todo.md` review section

- [ ] **Step 1: Run the macOS full suite.**

  Run `cd apps/macos && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test`.
  Expected: all existing tests and the new display/activity tests pass.

- [ ] **Step 2: Run shared and Windows tests on the supported runner.**

  Run `cd apps/linux && cargo test -p claude-dashboard-core`, then
  `cd apps/windows && cargo test --workspace` on Windows. Record separately any
  Windows GUI run-check that cannot run on macOS.

- [ ] **Step 3: Perform visual acceptance.**

  Verify setting off, setting on, 5h-only placement, 6/4 phase timing, 0.5 s
  transitions, 600 s stale hiding, failed refresh behaviour, light/dark themes,
  compact/full sizes, multiple cards, and no layout movement. Check CPU/memory
  with many cards and verify missing assets fall back to percent.

- [ ] **Step 4: Update task review evidence.**

  Record test commands/results, asset validation output, platform limitations, and
  the final commit ids in `tasks/todo.md`. Do not mark the feature complete until
  the Windows visual run-check has been performed.

