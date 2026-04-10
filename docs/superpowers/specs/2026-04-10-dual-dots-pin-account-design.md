# Dual Status Dots & Pin Account

**Date:** 2026-04-10

## Feature 1: Dual Status Dots

### Current
Each AccountCard header shows: `[Plan badge] [● 5h dot (10px)]`

### New
`[Plan badge] [● 5h dot (14px)] [● 7d dot (10px)]`

### Details
- 5h dot: 10px → **14px**, color from `usageColor(for: usage.fiveHour.utilization)`
- 7d dot: new **10px** circle, color from `usageColor(for: usage.sevenDay.utilization)`
- Both dots only shown when usage data is available (not loading/expired) — same conditional as current single dot
- Spacing: standard SwiftUI HStack spacing between the two dots

### Files Changed
- `ClaudeDashboard/Views/AccountCard.swift` — add second Circle, resize first

## Feature 2: Pin Account

### Model Change
Add `var isPinned: Bool = false` to `Account` struct. Codable auto-handles default for existing persisted data.

### Sort Logic
Pinned account always appears first. Remaining accounts sorted by burn rate (descending), same as current behavior.

```
[pinned account] → [rest sorted by burn rate ↓]
```

### Menu Bar Label
- If a pinned account exists with usage data → display its 5h stats
- Otherwise → fallback to current behavior (highest 5h utilization across all accounts)

### Pin/Unpin UI
- **Right-click context menu** on AccountCard: "Pin to Top" / "Unpin"
- Pinned card shows a small 📌 icon in the header (near the account name)
- Only 1 account can be pinned at a time — pinning a new account auto-unpins the previous one

### Pin Logic Location
- `DashboardViewModel` gets `func togglePin(for accountId: UUID)` which:
  1. Unpins all accounts
  2. If the target wasn't already pinned, pins it
  3. Re-sorts `accountStates`
  4. Persists via `AccountStore`

### Files Changed
- `ClaudeDashboard/Models/Account.swift` — add `isPinned` field
- `ClaudeDashboard/ViewModels/DashboardViewModel.swift` — sort logic, `menuBarLabel`, `togglePin()`
- `ClaudeDashboard/Views/AccountCard.swift` — context menu, pin icon
- `ClaudeDashboard/Services/AccountStore.swift` — no structural change needed (Codable handles it)

## Out of Scope
- Multiple pinned accounts
- Drag-and-drop reordering
- Pin from Settings view
