# Handoff: Burn animal animation

Date: 2026-10-27
Repository: `/Users/haicao/code/others/claude-dashboard`
User requested a pause and continuation in a new session.

## User-approved outcome

For both macOS and Windows:

- Only the large percentage ring of the 5-hour window may show the burn animal.
- 7-day, Fable, countdown rings, Overview legend, and Help sample stay without
  the animated animal.
- When `showBurnAnimals` is enabled, all eligible cards share one 10-second
  phase: 6 seconds of percentage and 4 seconds of animal, with 0.5-second
  cross-fades.
- An account is eligible only when its successful 5-hour utilization or reset
  cycle changed within the previous 600 seconds. Unchanged refreshes and failed
  refreshes do not extend that window.
- The running motion should look polished and physically coherent. Blender is
  allowed as a build-time tool; Photocraft is optional for reference only.
- User explicitly asked that feature files and generated assets live in the
  project directories. Blender itself may remain installed outside the project.

## Completed and committed

- `063c1b7c` — approved design spec:
  `docs/superpowers/specs/2026-10-27-burn-animal-animation-design.md`
- `4d8cefbc` — approved implementation plan:
  `docs/superpowers/plans/2026-10-27-burn-animal-animation.md`
- `578d1966` — Task 1 contract and pure models:
  - `contract/cases/animal-display.json`
  - `contract/README.md`
  - `apps/linux/core/src/animal_display.rs`
  - `apps/linux/core/src/lib.rs`
  - `apps/macos/ClaudeDashboard/Models/AnimalDisplayModels.swift`
  - `apps/macos/ClaudeDashboardTests/AnimalDisplayModelsTests.swift`

Task 1 Rust focused test passed:

```text
cd apps/linux && cargo test -p claude-dashboard-core animal_display
4 passed
```

Swift model and test file typecheck/parser checks passed. The focused XCTest did
not complete: the sandboxed run could not communicate with `testmanagerd`; the
escalated retry stalled with no output and was stopped after three unchanged
polls. No background test process was left.

## Current working tree

Expected status is clean except for the user-provided untracked `AGENTS.md`:

```text
?? AGENTS.md
```

Do not add or remove `AGENTS.md`.

## Task 2 state: Blender assets not finished

The active Task 2 subagent was stopped at the user's pause request. Initially
Blender was absent. Homebrew's official cask download hit HTTP 403/Cloudflare.
The subagent downloaded an accessible mirror copy here:

```text
/private/tmp/blender-5.2.3-macos-arm64.dmg
```

Observed size was about 366 MiB. It has not been mounted/installed and no asset
files were created yet. The command prefix for checking its SHA-256 was approved:

```bash
shasum -a 256 /private/tmp/blender-5.2.3-macos-arm64.dmg
```

Do not assume the mirror is trusted without checking the checksum/source. If the
DMG is used, install Blender only as a build-time tool. Keep all feature output in
the project:

- Blender source/scripts: `tools/burn-animal-animation/`
- macOS generated sheets/manifest:
  `apps/macos/ClaudeDashboard/Resources/BurnAnimals/`
- Windows generated sheets/manifest:
  `apps/windows/app/ui/assets/burn-animals/`

Task 2 must create and validate:

- `BLENDER_VERSION`
- `burn_animals.blend`
- `render.py`
- `validate_assets.py`
- `README.md`
- `manifest.json`
- five stable-id sheets: `snail`, `turtle`, `rabbit`, `horse`, `cheetah`
- each sheet RGBA PNG, 6x4 grid, 128x128 cells, 768x512 total, 24 frames,
  12 fps, max 2 MiB, alpha/bounds/loop checks from the plan.

## Next execution order

Use the user's chosen subagent-driven workflow, one task at a time:

1. Resume Task 2 asset pipeline. Verify/install Blender, create/render/validate
   the five sheets, review at 52 px and 68 px, commit the asset checkpoint.
2. Task 3: macOS `AnimalDisplayClock` and 600-second activity wiring in
   `DashboardViewModel`.
3. Task 4: macOS cached sprite renderer, fixed center ZStack, all caller wiring,
   remove legacy 7d/Fable/Overview/Help animal paths.
4. Task 5: Windows shared activity fields, stable asset catalog, refresh/model
   wiring and tests.
5. Task 6: Windows Slint global phase, clipped sprite, cross-fade, packaging and
   fallback; run the Windows fake-row visual check on Windows.
6. Task 7: full macOS tests, Linux core tests, Windows workspace tests, visual
   acceptance, and update `tasks/todo.md` review evidence.

After each task: inspect the diff, run the focused command from the plan, record
platform limitations, and commit a checkpoint. Do not start the next task while
the current task has unreviewed changes.

## Important implementation constraints

- Stable asset ids must map from the existing emoji values; never use localized
  emoji glyphs as filenames.
- Keep burn-rate projection/sorting unchanged. The activity tracker controls
  display eligibility only.
- Use one process-wide/app-wide phase. Never create a timer per card.
- Missing or malformed assets/manifests must fall back to the percentage without
  crashing or blanking the gauge.
- The user asked for real-looking motion, so do not replace the final assets
  with static emoji or placeholder boxes merely to make tests pass.

