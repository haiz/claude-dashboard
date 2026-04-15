# UX Improvements: First Launch, Empty State, Dynamic Dock Icon

## Summary

Three UX improvements for the Claude Dashboard macOS menu bar app:

1. **First launch**: Auto-open dashboard on first app launch
2. **Empty state**: Hide irrelevant buttons and show "Add Account" CTA when no accounts exist
3. **Dynamic dock icon**: Show dock icon when dashboard is open, hide when closed

All changes follow inline approach — modifying existing files with minimal additions.

## Feature 1: First Launch Auto-Open Dashboard

### Behavior

- On first-ever app launch, dashboard window opens automatically
- Since `accounts.isEmpty` on first launch, SetupView sheet is also presented (existing logic)
- Subsequent launches behave normally (menu bar only)
- Uses `hasLaunchedBefore` UserDefaults flag (not tied to account count)

### Implementation

**File:** `ClaudeDashboard/ClaudeDashboardApp.swift`

- Add `.onAppear` modifier inside `MenuBarExtra` body
- Check `UserDefaults.standard.bool(forKey: "hasLaunchedBefore")`
- If `false`: call `appDelegate.openDashboardWindow(viewModel:)`, then set flag to `true`
- The existing `openDashboardWindow` logic already presents SetupView when `accounts.isEmpty`

### UserDefaults Key

- `"claude-dashboard.hasLaunchedBefore"` — Boolean, default `false`

## Feature 2: Empty State — Hide Refresh, Show Add Account CTA

### Behavior

When `viewModel.accountStates.isEmpty`:

**MenuBarPopover:**
- Hide the refresh button (`arrow.clockwise`) in the header
- Replace text-only empty state with an actionable "Add Account" button
- Button opens dashboard window + SetupView sheet simultaneously (same as first launch flow)

**DashboardWindow:**
- Hide Refresh and Overview buttons in the toolbar (Settings button remains)
- Replace text-only empty state with an actionable "Add Account" button
- Button presents SetupView sheet directly on the dashboard

### Implementation

**File:** `ClaudeDashboard/Views/MenuBarPopover.swift`

- Wrap refresh button in `if !viewModel.accountStates.isEmpty { ... }`
- Add button to `emptyState` view that calls `onOpenWindow()` then closes popover (reuses existing expand button pattern)

**File:** `ClaudeDashboard/Views/DashboardWindow.swift`

- Wrap Refresh and Overview buttons in `if !viewModel.accountStates.isEmpty { ... }`
- Add `@State private var showingSetup = false`
- Add `.sheet(isPresented: $showingSetup)` presenting `SetupView`
- Add "Add Account" button in `emptyStateView` that sets `showingSetup = true`

## Feature 3: Dynamic Dock Icon

### Behavior

- App starts in `.accessory` mode (menu bar only, no dock icon) — `LSUIElement = true` unchanged
- When dashboard window opens: switch to `.regular` (dock icon appears)
- When dashboard window closes/hides: switch back to `.accessory` (dock icon disappears)
- Clicking dock icon while dashboard is visible brings window to foreground

### Implementation

**File:** `ClaudeDashboard/ClaudeDashboardApp.swift` — `AppDelegate`

**`openDashboardWindow(viewModel:)`:**
- Add `NSApp.setActivationPolicy(.regular)` before `makeKeyAndOrderFront(nil)`

**`windowShouldClose(_:)`:**
- Add `NSApp.setActivationPolicy(.accessory)` after `orderOut(nil)`
- Order matters: hide window first, then change policy

**`applicationShouldHandleReopen(_:hasVisibleWindows:)` (new):**
- If dashboard window exists and is not visible, re-open it (bring to front)
- Safety handler for dock icon click edge case

### Info.plist

- `LSUIElement = true` remains unchanged — this sets the initial policy to `.accessory`

## Files Changed

| File | Changes |
|------|---------|
| `ClaudeDashboard/ClaudeDashboardApp.swift` | First launch check in `.onAppear`, dock policy toggle in `openDashboardWindow` and `windowShouldClose`, new `applicationShouldHandleReopen` |
| `ClaudeDashboard/Views/MenuBarPopover.swift` | Conditional refresh button, actionable empty state with "Add Account" button |
| `ClaudeDashboard/Views/DashboardWindow.swift` | Conditional Refresh/Overview buttons, `showingSetup` state + sheet, actionable empty state |

## Edge Cases

- **User deletes all accounts then restarts**: Dashboard does NOT auto-open (flag is already `true`). Empty state is shown in popover and dashboard with "Add Account" button.
- **Rapid dock icon click during policy transition**: `applicationShouldHandleReopen` handles this safely.
- **Window minimize vs close**: Only close (red X) triggers policy change. Minimize keeps dock icon visible (standard macOS behavior).
- **Multiple sheets**: DashboardWindow can now present both Settings and Setup sheets. Only one can be active at a time (SwiftUI standard behavior).
