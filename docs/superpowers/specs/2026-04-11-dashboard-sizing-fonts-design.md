# Dashboard Sizing & Font Increase Design

**Date:** 2026-04-11
**Status:** Approved

## Goal

Make the dashboard larger and card text more readable by:
1. Increasing window size to 1050x750 (1.5x from 700x500)
2. Switching card layout from multi-column grid to single-column full-width
3. Bumping font sizes inside AccountCard and UsageBar

## Changes

### 1. Window Size (ClaudeDashboardApp.swift)

| Property | Current | New |
|----------|---------|-----|
| Initial size | `NSRect(width: 700, height: 500)` | `NSRect(width: 1050, height: 750)` |
| Min size | `NSSize(width: 400, height: 300)` | `NSSize(width: 600, height: 450)` |

### 2. Min Frame (DashboardWindow.swift)

| Property | Current | New |
|----------|---------|-----|
| `.frame(minWidth:minHeight:)` | `400, 300` | `600, 450` |

### 3. Card Layout (DashboardWindow.swift)

Replace `LazyVGrid` with adaptive grid items:
```swift
LazyVGrid(
    columns: [GridItem(.adaptive(minimum: 280, maximum: 400), spacing: 12)],
    spacing: 12
)
```

With a simple vertical stack:
```swift
LazyVStack(spacing: 12)
```

Each card takes the full width of the scroll area. One card per row.

### 4. Font Bumps — AccountCard.swift

| Element | Current | New |
|---------|---------|-----|
| Account name | `.headline` | `.title3` |
| Email subtitle | `.caption` | `.subheadline` |
| Plan badge text | `.caption2.bold()` | `.caption.bold()` |
| Error message | `.caption` | `.subheadline` |

### 5. Font Bumps — UsageBar.swift

| Element | Current | New |
|---------|---------|-----|
| Label (5h, 7d, S) | `.system(.caption, design: .monospaced)` | `.system(.body, design: .monospaced)` |
| Percentage text | `.system(.caption, design: .monospaced)` | `.system(.body, design: .monospaced)` |
| Reset time text | `.system(.caption, design: .monospaced)` | `.system(.body, design: .monospaced)` |

### Files Affected

1. `ClaudeDashboard/ClaudeDashboardApp.swift` — window initial + min size
2. `ClaudeDashboard/Views/DashboardWindow.swift` — minWidth/minHeight + grid to stack
3. `ClaudeDashboard/Views/AccountCard.swift` — font bumps
4. `ClaudeDashboard/Views/UsageBar.swift` — font bumps

### Out of Scope

- MenuBarPopover sizing (stays at 320pt width)
- SetupView / SettingsView sheet sizes
- DashboardWindow toolbar fonts
- OverviewChartView / AccountDetailView fonts
