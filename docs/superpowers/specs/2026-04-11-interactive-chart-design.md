# Interactive Chart Enhancements

**Date:** 2026-04-11
**Scope:** OverviewChartView + AccountDetailView
**Approach:** Reusable InteractiveChartContainer wrapper (Approach B)

## Overview

Add interactive zoom, pan, time range presets, and average rate display to both chart views. Extract shared interaction logic into a reusable `InteractiveChartContainer` that wraps chart content.

## 1. InteractiveChartContainer

New generic wrapper view `InteractiveChartContainer<ChartContent: View>`.

### State
- `visibleRange: ClosedRange<Date>` — currently displayed time window
- `mode: InteractionMode` — `.pan` or `.zoom`, default `.pan`
- `selectedPreset: TimeRangePreset?` — nil when in custom range
- `zoomSelectionRect: CGRect?` — highlight rect during zoom drag

### Layout
```
┌─────────────────────────────────────────────────┐
│ [🤚/🔍] [5h] [24h] [3d] [7d] [30d] [Custom]  │  ← Toolbar row
│ [... extra controls from caller ...]            │
├─────────────────────────────────────────────────┤
│                                   ┌───────────┐ │
│   Chart content (from caller)     │ Avg: 12%/h│ │  ← Overlay
│                                   └───────────┘ │
│   + gesture layer (pan or zoom-select)          │
└─────────────────────────────────────────────────┘
```

### Caller provides
- Chart content (SwiftUI Chart lines/marks)
- Data points for average rate calculation
- Extra toolbar items (window picker, legend toggles, etc.)
- `onRangeChanged: (ClosedRange<Date>) -> Void` callback

### TimeRangePreset enum
```swift
enum TimeRangePreset: String, CaseIterable {
    case fiveHour = "5h"     // 18000s
    case day = "24h"         // 86400s
    case threeDay = "3d"     // 259200s
    case week = "7d"         // 604800s
    case month = "30d"       // 2592000s
}
```

Shared by both charts. Replaces the existing `TimeRange` enum in OverviewChartView.

## 2. Gesture & Interaction Behavior

### Mode Toggle
- Button on toolbar, icon changes: hand.raised (pan) / magnifyingglass (zoom)
- Default mode: pan

### Pan Mode
- Drag horizontally on chart → shift `visibleRange` left/right
- Duration stays constant (e.g., if viewing 24h, pan keeps 24h window)
- Clamped to data bounds: earliest log entry → Date()

### Zoom Mode
- Drag horizontally on chart → draw selection highlight (X-axis only, full chart height)
- On release → `visibleRange` zooms to selected interval
- Minimum zoom: 5 minutes (prevents zooming into empty space)
- Selection highlight: semi-transparent accent color overlay

### Preset Buttons
- Click preset → `visibleRange` resets to that duration ending at `Date()`
- `selectedPreset` set to that preset
- Any pan or zoom that changes range → `selectedPreset = nil`, "Custom" label appears
- "Custom" label is indicator only, not clickable

### Scroll Wheel (optional/bonus)
- Scroll up = zoom in (narrow range around center point)
- Scroll down = zoom out (widen range around center point)
- Skip if implementation is too complex for initial version

## 3. Average Rate Overlay

### Position
Top-right corner inside chart area. Background: `.ultraThinMaterial`, rounded corners, small padding.

### Content
```
Avg: 12.3%/h
```

### Calculation
- Take all data points within current `visibleRange`
- Only consider consecutive pairs where utilization is non-decreasing (skip reset drops where utilization falls by >10% between consecutive points — these are cycle resets, not consumption)
- Sum the positive deltas across qualifying pairs, divide by total hours of the visible range
- `avgRate = sumOfPositiveDeltas / totalVisibleHours`
- Positive rate (consuming) → orange/red text
- Zero or near-zero rate → green text
- Insufficient data (< 2 points) → display "Avg: --"

### Per-chart behavior
- **OverviewChartView:** avg rate of the total line (weighted average across accounts)
- **AccountDetailView:** avg rate of the single account being viewed

### Updates
Recalculated whenever `visibleRange` changes (pan, zoom, preset click).

## 4. Reset Cycles Collapsed (AccountDetailView only)

### Change
- Default state: **collapsed** — only header row visible
- Click header → expand to show full cycle list
- Existing behavior preserved: click cycle to filter chart, color dots, peak %, data point count

### Implementation
- `@State var cyclesExpanded = false`
- Header: `HStack { Text("Reset Cycles") .font(.caption.bold()), Spacer(), Image(systemName: chevron) }`
- Chevron: `chevron.right` when collapsed, `chevron.down` when expanded
- Content wrapped in `if cyclesExpanded { ... }`
- Animation: `.easeInOut(duration: 0.2)` on `cyclesExpanded`

No interaction with InteractiveChartContainer — lives outside and below it.

## 5. Integration

### OverviewChartView
- Wrap existing chart content in `InteractiveChartContainer`
- Toolbar extras: window picker (5h/7d/Sonnet) + legend toggles
- Remove old `TimeRange` enum and picker
- Avg rate source: `totalLine` (weighted average)
- `onRangeChanged` → reload logs from `UsageLogStore` with new date bounds

### AccountDetailView
- Wrap existing chart content in `InteractiveChartContainer`
- Toolbar extras: window picker (5h/7d/Sonnet) + "Show All" button
- Gains time range presets (currently has none)
- Avg rate source: single account logs
- `onRangeChanged` → reload logs
- Reset cycles section placed outside container, below it, collapsed by default

### Data flow
```
Preset click / Pan / Zoom
       ↓
visibleRange changes
       ↓
onRangeChanged callback fires
       ↓
Parent reloads logs: UsageLogStore.logs(from: range.lower, to: range.upper)
       ↓
Chart re-renders + avg rate recalculates
```

## 6. File Changes

| File | Action | Description |
|------|--------|-------------|
| `InteractiveChartContainer.swift` | New | ~200-250 LOC, reusable chart wrapper |
| `OverviewChartView.swift` | Modify | Replace time range picker, wrap in container |
| `AccountDetailView.swift` | Modify | Wrap in container, collapse reset cycles |
| `project.yml` | Modify | Add new file to sources |

The existing `TimeRange` enum in OverviewChartView is replaced by the shared `TimeRangePreset` enum defined in or alongside `InteractiveChartContainer`.
