# Dual Status Dots & Pin Account — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a 7d status dot alongside the existing 5h dot on each card, and allow pinning one account to always appear first + drive the menu bar label.

**Architecture:** Two independent features touching the same files. Feature 1 (dual dots) is view-only. Feature 2 (pin) threads through Model → ViewModel → View. Implement dual dots first since it's simpler and has no model changes.

**Tech Stack:** Swift, SwiftUI, Codable (UserDefaults persistence)

---

### Task 1: Add 7d status dot to AccountCard header

**Files:**
- Modify: `ClaudeDashboard/Views/AccountCard.swift:38-42`

- [ ] **Step 1: Replace single dot with dual dots**

In `AccountCard.swift`, replace the current single Circle block (lines 38-42):

```swift
} else if let usage = state.usage {
    Circle()
        .fill(DashboardViewModel.usageColor(for: usage.fiveHour.utilization))
        .frame(width: 10, height: 10)
}
```

With:

```swift
} else if let usage = state.usage {
    Circle()
        .fill(DashboardViewModel.usageColor(for: usage.fiveHour.utilization))
        .frame(width: 14, height: 14)
    Circle()
        .fill(DashboardViewModel.usageColor(for: usage.sevenDay.utilization))
        .frame(width: 10, height: 10)
}
```

The two circles sit naturally in the existing `HStack` — no wrapper needed.

- [ ] **Step 2: Build and verify**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `BUILD SUCCEEDED`

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/AccountCard.swift
git commit -m "feat: add 7d status dot alongside 5h dot in card header"
```

---

### Task 2: Add `isPinned` field to Account model

**Files:**
- Modify: `ClaudeDashboard/Models/Account.swift:26-41`

- [ ] **Step 1: Add isPinned property**

In `Account.swift`, add `isPinned` after the `status` field (line 36):

```swift
struct Account: Identifiable, Codable, Equatable {
    let id: UUID
    var name: String
    var email: String?
    var chromeProfilePath: String
    var chromeProfileName: String?
    var orgId: String?
    var sessionKey: String?
    var plan: AccountPlan
    var lastSynced: Date?
    var status: AccountStatus
    var isPinned: Bool = false

    var isConfigured: Bool {
        orgId != nil
    }
}
```

The `= false` default means `Codable` will decode existing persisted accounts without this field as `isPinned: false` — no migration needed.

- [ ] **Step 2: Build and verify**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `BUILD SUCCEEDED`

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Models/Account.swift
git commit -m "feat: add isPinned field to Account model"
```

---

### Task 3: Add togglePin to DashboardViewModel

**Files:**
- Modify: `ClaudeDashboard/ViewModels/DashboardViewModel.swift`

- [ ] **Step 1: Add togglePin method**

Add this method after `resyncAccount` (after line 144) in `DashboardViewModel.swift`:

```swift
// MARK: - Pin

func togglePin(for accountId: UUID) {
    // Unpin all accounts first
    for account in accountStore.accounts {
        if account.isPinned {
            var updated = account
            updated.isPinned = false
            accountStore.updateAccount(updated)
        }
    }

    // Pin the target if it wasn't already pinned
    if let account = accountStore.accounts.first(where: { $0.id == accountId }),
       !accountStates.first(where: { $0.id == accountId })?.account.isPinned ?? false {
        // Re-read account after unpin pass (it was just updated)
        if var target = accountStore.accounts.first(where: { $0.id == accountId }) {
            target.isPinned = true
            accountStore.updateAccount(target)
        }
    }
}
```

Wait — the toggle logic is simpler if we check the pre-toggle state. Let's use this cleaner version:

```swift
// MARK: - Pin

func togglePin(for accountId: UUID) {
    let wasPinned = accountStore.accounts.first(where: { $0.id == accountId })?.isPinned ?? false

    // Unpin all accounts
    for account in accountStore.accounts where account.isPinned {
        var updated = account
        updated.isPinned = false
        accountStore.updateAccount(updated)
    }

    // If it wasn't pinned before, pin it now
    if !wasPinned, var target = accountStore.accounts.first(where: { $0.id == accountId }) {
        target.isPinned = true
        accountStore.updateAccount(target)
    }
}
```

- [ ] **Step 2: Update sort logic to respect pinned**

Replace the sort on line 115 (`accountStates.sort { ... }` inside `refreshAll()`):

```swift
accountStates.sort {
    if $0.account.isPinned != $1.account.isPinned { return $0.account.isPinned }
    return DashboardViewModel.burnRate(for: $0) > DashboardViewModel.burnRate(for: $1)
}
```

Replace the identical sort on line 210 (`syncStates(with:)` method):

```swift
accountStates.sort {
    if $0.account.isPinned != $1.account.isPinned { return $0.account.isPinned }
    return DashboardViewModel.burnRate(for: $0) > DashboardViewModel.burnRate(for: $1)
}
```

- [ ] **Step 3: Update menuBarLabel to use pinned account**

Replace the current `menuBarLabel` computed property (lines 148-165):

```swift
var menuBarLabel: String {
    // Prefer pinned account's usage, fallback to highest 5h utilization
    let source: UsageLimit? = {
        if let pinned = accountStates.first(where: { $0.account.isPinned }),
           let usage = pinned.usage {
            return usage.fiveHour
        }
        return accountStates
            .compactMap { $0.usage?.fiveHour }
            .max(by: { $0.utilization < $1.utilization })
    }()

    guard let limit = source else { return "--" }

    let pct = Int(limit.utilization)
    if let reset = limit.resetsAt {
        let remaining = reset.timeIntervalSinceNow
        if remaining > 0 {
            let h = Int(remaining) / 3600
            let m = (Int(remaining) % 3600) / 60
            return "\(pct)% \u{00B7} \(h)h\(String(format: "%02d", m))m"
        }
    }
    return "\(pct)%"
}
```

- [ ] **Step 4: Build and verify**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `BUILD SUCCEEDED`

- [ ] **Step 5: Commit**

```bash
git add ClaudeDashboard/ViewModels/DashboardViewModel.swift
git commit -m "feat: add togglePin, pinned-first sorting, and pinned menu bar label"
```

---

### Task 4: Add pin UI to AccountCard (context menu + pin icon)

**Files:**
- Modify: `ClaudeDashboard/Views/AccountCard.swift`

- [ ] **Step 1: Add onTogglePin callback and context menu**

Update the struct to accept a new callback and EnvironmentObject, and add context menu + pin icon. Replace the entire `AccountCard.swift`:

The struct needs a new callback. Add it after `onResync`:

```swift
struct AccountCard: View {
    let state: AccountUsageState
    let onResync: () -> Void
    let onTogglePin: () -> Void
```

Add a pin icon in the header, right after the account name VStack and before Spacer. Insert this between the name VStack closing brace (line 20) and `Spacer()` (line 22):

```swift
if state.account.isPinned {
    Image(systemName: "pin.fill")
        .font(.caption)
        .foregroundStyle(.secondary)
}
```

Add a `.contextMenu` modifier to the outermost `GroupBox`. After the closing `}` of the GroupBox (line 56, after `.padding(.vertical, 4)`), add:

```swift
.contextMenu {
    Button {
        onTogglePin()
    } label: {
        Label(
            state.account.isPinned ? "Unpin" : "Pin to Top",
            systemImage: state.account.isPinned ? "pin.slash" : "pin"
        )
    }
}
```

- [ ] **Step 2: Update all AccountCard call sites**

In `MenuBarPopover.swift`, find the `AccountCard` initializer (should look like `AccountCard(state: state) {`). Add the `onTogglePin` parameter:

```swift
AccountCard(state: state, onResync: {
    Task { await viewModel.resyncAccount(state.id) }
}, onTogglePin: {
    viewModel.togglePin(for: state.id)
})
```

In `DashboardWindow.swift`, find the same `AccountCard` initializer and update identically:

```swift
AccountCard(state: state, onResync: {
    Task { await viewModel.resyncAccount(state.id) }
}, onTogglePin: {
    viewModel.togglePin(for: state.id)
})
```

- [ ] **Step 3: Build and verify**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `BUILD SUCCEEDED`

- [ ] **Step 4: Run all tests**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | tail -20
```
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add ClaudeDashboard/Views/AccountCard.swift ClaudeDashboard/Views/MenuBarPopover.swift ClaudeDashboard/Views/DashboardWindow.swift
git commit -m "feat: add pin/unpin context menu and pin icon to AccountCard"
```

---

### Task 5: Final build + xcodegen sync

**Files:**
- Possibly: `project.yml` (only if new files were added — none were in this plan)

- [ ] **Step 1: Full build**

```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `BUILD SUCCEEDED`

- [ ] **Step 2: Run tests**

```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | tail -20
```
Expected: All tests pass.

- [ ] **Step 3: Final commit if any fixups needed**

Only if previous steps required changes.
