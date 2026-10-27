# Burn-rate animal animation in the 5h gauge

## Status

Approved design, ready for spec review. This document describes the cross-platform
behaviour and the asset/runtime boundaries. It does not implement the feature.

## Goal

When Settings > General > Show burn-rate animals is enabled, the large percentage
ring for an account's 5-hour window alternates between the percentage and the
burn-rate animal. The percentage remains the source of truth and the accessible
value for the gauge; the visible center alternates between the percentage and the
animal. The animal is shown only while the account's 5-hour usage has changed
recently.

The animation must look like a real, polished locomotion cycle at the small
52 px compact and 68 px full-size gauges. macOS and Windows use the same phase,
visibility rules, frame cadence, and animal mapping.

## Scope and non-goals

In scope:

- macOS account cards, dashboard/account pages, and menu-bar popover gauges.
- Windows dashboard cards, account page, and flyout gauges.
- The existing five burn-rate animals: snail, turtle, rabbit, horse, and cheetah.
- A shared 10-second display phase for all visible cards in one app process.
- A 600-second activity timeout based on successful 5-hour usage observations.
- Blender source files and a deterministic render/export script for the final art.

Out of scope:

- Changing burn-rate calculation, sorting, thresholds, or usage-log semantics.
- Showing an animal in the 7-day, Fable, or countdown rings.
- Showing an animal in the Overview legend or Help sample gauge.
- Adding Photocraft or Blender as an application runtime dependency.
- Persisting animation phase or activity state across app launches.

The existing `showBurnAnimals` setting remains off by default. Reset All Settings
continues to write `false`; this feature adds no settings migration and does not
change the existing key or persistence format.

## Behaviour contract

### Eligibility

An animal may be rendered only when all conditions hold:

1. `showBurnAnimals` is true.
2. The gauge is the large percentage ring for the 5-hour window.
3. The account has a current burn-rate animal from the existing tracker.
4. The account has a successful 5-hour usage observation whose utilization or
   reset cycle changed within the last 600 seconds.

The setting remains display-only. Burn-rate tracking, sorting, and usage logging
continue when it is false.

The activity tracker is keyed by account id. A successful observation changes the
activity timestamp when either:

- the 5-hour utilization differs from the previous successful observation; or
- the 5-hour `resetsAt` moves to a different reset cycle under the existing
  60-second reset tolerance.

An unchanged observation does not extend the timestamp. A failed refresh, an
expired account, or a missing usage response does not extend it. The first
observation establishes a baseline and does not create a burn-rate result by
itself, matching the existing tracker behaviour.

### Shared phase

One process-wide display clock is exposed to all gauge views. Its phase is the
monotonic elapsed time since the clock was created, modulo 10 seconds:

| Phase | Content |
| --- | --- |
| 0.0 s to 5.5 s | Percentage fully visible |
| 5.5 s to 6.0 s | Percentage to animal cross-fade |
| 6.0 s to 9.5 s | Animal fully visible |
| 9.5 s to 10.0 s | Animal to percentage cross-fade |

The phase is shared by every eligible card. A card that becomes eligible joins
the current phase; it does not start a private cycle. When the setting is off or
no card is eligible, the clock must not create per-card timers or retain a frame
loop for hidden content.

The two boundary intervals are visual transition boundaries. Each is exactly
500 ms, so the outgoing and incoming layers overlap while remaining inside the
fixed center region. At phase 0.0 the animal-to-percentage transition is complete;
the modulo boundary does not cause a flash. The number and animal use the same
frame and alignment box, so the ring and its layout never move.

### Animal motion

The art is authored as a stylized but physically coherent locomotion cycle:

- snail: smooth gliding body and antenna motion;
- turtle: alternating leg motion and shell/body weight shift;
- rabbit: compressed and extended hop cycle;
- horse: readable gallop with head/torso counter-motion;
- cheetah: extended sprint stride with tail counterbalance.

The target is a two-second loop rendered at 12 fps, 24 frames per animal. The
four-second animal phase plays the loop twice. The frame cadence is global for
all cards and animals. The animation should preserve silhouette and eye-catching
motion at 52 px; extra detail that disappears at that size is not a priority.

Frame selection is defined from the shared phase so both platforms render the
same motion: during phase 5.5–6.0 the entering animal holds frame 0; during
phase 6.0–10.0 the frame is
`floor(((phase - 6.0) * 12) modulo 24)`. Thus the four-second animal interval
runs exactly two 24-frame cycles. During phase 9.5–10.0 the animal continues
through frames 18–23 while it fades out; at the modulo boundary the percentage
is fully visible and the animal frame is no longer sampled.

The animal layer is decorative. Its accessibility label is omitted or marked
decorative; the percentage remains the accessible value for the gauge.

## Asset pipeline

### Source

The repository will contain a small Blender source project and a headless render
script under a dedicated animation asset directory. A checked-in
`BLENDER_VERSION` file pins the exact executable version used for final renders;
the render script fails when the installed Blender version does not match it.
The script fixes scene resolution, orthographic camera, lighting, transparent
film, frame range, color management, and output naming. The generated assets are
reproducible from the source project without installing anything into the app.

Photocraft may be used to explore silhouettes or references. Its output is not a
runtime dependency and is not the source of truth for final frames.

### Output

Each animal is exported as a transparent RGBA PNG sprite sheet with a 6-by-4
grid of 128-by-128 px cells (768-by-512 px per sheet). The sheet includes enough
padding for the 68 px full-size gauge and is validated at both gauge sizes. A
manifest records animal name, cell dimensions, frame count, frame rate, Blender
version, and source revision.

The build checks:

- exactly 24 frames per animal;
- exact 128-by-128 px cells and a 768-by-512 px sheet;
- an alpha channel and no opaque pixels outside the subject padding box;
- each frame's non-transparent bounding box stays within the 112-by-112 px
  subject area, with its center no more than 8 px from the cell center;
- first/last frame alpha-area difference under 10% and subject-center shift under
  4 px, so the two-second loop does not jump;
- each sheet is at most 2 MiB;
- deterministic file names and manifest entries.

If an asset cannot be loaded at runtime, the gauge falls back to the percentage
layer and does not crash or blank the ring.

## macOS design

Add pure display models for:

- `AnimalDisplayPhase`: phase, content mode, transition progress, and frame index;
- `AnimalActivityTracker`: account-keyed last-change timestamps and eligibility.

The view model owns one display clock and publishes its phase. `refreshAll` feeds
only successful 5-hour observations into the activity tracker. The existing
`BurnRateTracker` remains responsible for projection and animal selection.

`UsageGaugeRow` passes an animation payload only for its 5-hour `UsageBar`.
`CircularProgressView` keeps the percentage and animated animal in a fixed `ZStack`
inside the existing ring. The old bottom-right emoji overlay is removed from the
5-hour path. The 7-day and Fable paths keep their existing inputs and layout.

The sprite sheet is decoded and cached once per animal. Frame selection uses the
shared phase and does not decode image data during every refresh. The view keeps
the ring's existing hover and click behaviour.

## Windows design

Port the pure phase and activity rules into the shared Rust/core layer so the
boundary cases are tested once in the Windows implementation and mirrored in
Swift tests. The row model carries eligibility and the 5-hour animation payload;
7-day and Fable data remain unchanged.

The Slint view adds an animated animal layer inside the large 5-hour `WindowColumn`
ring only. The frame is selected from the sprite sheet using image source clipping,
and opacity/scale transitions are animated over 500 ms. One UI clock or animation
tick drives all visible gauges. Existing one-second refresh and row publishing
remain responsible for data, not animation frame timing.

Slint supports image source clipping for sprite sheets and native timers/animation
ticks, so no third-party animation runtime is needed.

## Failure and lifecycle rules

- Refresh failures do not reset or extend animal activity.
- A reset cycle or utilization decrease follows existing burn-rate reset rules;
  the activity timestamp may update for a new reset cycle, but no animal is shown
  until the existing burn-rate tracker supplies a valid result.
- Disabling the setting immediately removes animal layers and stops hidden frame
  work. Re-enabling uses the current in-memory activity timestamp.
- App launch starts a fresh activity baseline, as the existing in-memory burn-rate
  tracker does. No historical usage log is used to make a card appear active.
- Missing or malformed asset manifests use the percentage-only fallback.

## Testing and acceptance

### Pure tests

- phase at 0, 5.5, 6, 9.5, and 10 seconds;
- 500 ms transition progress in both directions;
- frame index wrapping and stable shared phase;
- activity updates on utilization change and reset-cycle change;
- activity not updated by unchanged observations or failed refreshes;
- exactly 600 seconds eligible and just beyond 600 seconds hidden;
- account isolation;
- setting off, missing burn result, and non-5-hour windows never eligible.

### Asset tests

- manifest and sprite-sheet dimensions;
- alpha/background validation;
- frame count and naming;
- deterministic headless export.

### UI verification

- macOS compact and full gauges in light/dark appearance;
- Windows dashboard, account page, and flyout at multiple DPI scales;
- multiple cards visibly changing phase together;
- smooth number/animal transitions with no ring or layout movement;
- stale cards retaining percent/ring while hiding only the animal;
- CPU and memory check with many visible cards;
- setting toggle and refresh error behaviour.

## Rollout order

1. Add the pure contract and tests.
2. Build and validate Blender assets.
3. Implement macOS display model and UI.
4. Implement Windows model and Slint UI.
5. Run platform tests and visual checks.
6. Review the generated diff and update contract/help text only where the user
   visible behaviour changed.

