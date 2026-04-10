# Reset Countdown Bars Design

## Overview

Add visual countdown bars below each usage progress bar in `UsageBar.swift`. Each bar segment represents a time unit (1 hour for 5h limit, 1 day for 7d limit) and depletes from left to right as time passes, giving an intuitive sense of how much reset time remains.

## Layout

Each `UsageBar` becomes two rows sharing the same grid columns:

```
[label 24px] [progress bar ─────────────────────] [% 50px]
[     24px ] [      countdown bars (right-aligned)] [time 50px]
```

- **Row 1**: Existing progress bar (unchanged)
- **Row 2**: Countdown bars + reset time text
- Right edge of countdown bars aligns with right edge of progress bar
- Text column (50px, right-aligned) shared by "45%" and "6d 12h"

## Countdown Bars Spec

### Dimensions
- Total width: 2/3 of the progress bar area width
- Right-aligned within the progress bar column
- Height: 8px (same as progress bar)
- Corner radius: 2px per segment
- Gap between segments: 2px

### Segment Count
- **5h limit** (`totalSeconds == 18000`): 5 segments (1 per hour)
- **7d limit** (`totalSeconds == 604800`): 7 segments (1 per day)
- **Sonnet 7d limit**: Same as 7d — 7 segments

### Color
- **Remaining time**: Blue (`#4a90d9` / `Color.blue.opacity(0.7)`)
- **Elapsed time**: Muted background (`Color.primary.opacity(0.08)`)

### Depletion Logic

Given `resetsAt: Date` and `totalSeconds: TimeInterval`:

1. `remaining = resetsAt.timeIntervalSinceNow`
2. `elapsed = totalSeconds - remaining`
3. `secondsPerSegment = totalSeconds / segmentCount`
4. For each segment `i` (0-indexed, left to right):
   - `segmentStart = i * secondsPerSegment`
   - `segmentEnd = (i + 1) * secondsPerSegment`
   - If `elapsed >= segmentEnd`: segment fully depleted (muted)
   - If `elapsed <= segmentStart`: segment fully remaining (blue)
   - Otherwise: partially depleted — left portion muted, right portion blue
   - `fillFraction = (segmentEnd - elapsed) / secondsPerSegment`

### Text Display
- Same font as percentage: `.system(.caption, design: .monospaced)`
- Format: `"6d 12h"`, `"3h 20m"`, `"45m"` (same `formatTimeRemaining` logic)
- Color: `Color.secondary.opacity(0.6)` (or existing `resetUrgencyColor`)
- Width: 50px, right-aligned, `white-space: nowrap` equivalent

### Edge Cases
- `resetsAt == nil`: No countdown row displayed
- `remaining <= 0`: All segments depleted, text shows "now"
- `remaining >= totalSeconds`: All segments fully blue

## Scope
- **5h limit**: 5 bars, each = 1 hour
- **7d limit**: 7 bars, each = 1 day
- **Sonnet (S) limit**: 7 bars, each = 1 day
- Replaces existing "resets in X" text line (same info, better format)

## Files Modified
- `ClaudeDashboard/Views/UsageBar.swift` — add countdown bar row below progress bar

## No Changes To
- `AccountCard.swift` — already passes `resetsAt` and `totalSeconds` to `UsageBar`
- `UsageData` model — no new data needed
- `DashboardViewModel` — no new logic needed
