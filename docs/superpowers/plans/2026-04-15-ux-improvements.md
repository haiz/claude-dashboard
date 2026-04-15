# UX Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve first-run experience (auto-open dashboard), empty-state UX (hide refresh, show "Add Account" CTA), and dock icon lifecycle (appear when dashboard is open, disappear when closed).

**Architecture:** Inline modifications to three existing files. No new files or abstractions. `AppDelegate` gains dock-policy toggling and first-launch detection. Two view files gain conditional rendering and actionable empty states.

**Tech Stack:** SwiftUI, AppKit (NSApplication activation policy, NSWindow), UserDefaults

---

## File Map

| File | Responsibility | Changes |
|------|---------------|---------|
| `ClaudeDashboard/ClaudeDashboardApp.swift` | App entry, AppDelegate, window lifecycle | First-launch `.onAppear`, dock policy in `openDashboardWindow` / `windowShouldClose`, new `applicationShouldHandleReopen` |
| `ClaudeDashboard/Views/MenuBarPopover.swift` | Menu bar popover UI | Conditional refresh button, "Add Account" button in empty state |
| `ClaudeDashboard/Views/DashboardWindow.swift` | Dashboard window UI | Conditional Refresh/Overview buttons, `showingSetup` state + sheet, "Add Account" button in empty state |

---

### Task 1: Dynamic Dock Icon

**Files:**
- Modify: `ClaudeDashboard/ClaudeDashboardApp.swift`

- [ ] **Step 1: Add `.regular` policy when opening dashboard**

In `openDashboardWindow(viewModel:)`, add one line before `window.makeKeyAndOrderFront(nil)`:

```swift
    func openDashboardWindow(viewModel: DashboardViewModel) {
        currentViewModel = viewModel
        let showSetup = viewModel.accountStore.accounts.isEmpty
        let contentView = DashboardWindowWrapper(viewModel: viewModel, showSetupOnAppear: showSetup)

        let window: NSWindow
        if let existing = dashboardWindow {
            window = existing
            // Dismiss any lingering sheets from a previous session.
            while let sheet = window.attachedSheet {
                window.endSheet(sheet)
            }
        } else {
            window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 1050, height: 750),
                styleMask: [.titled, .closable, .miniaturizable, .resizable],
                backing: .buffered,
                defer: false
            )
            window.title = "Claude Dashboard"
            window.center()
            window.setFrameAutosaveName("ClaudeDashboardWindow")
            window.minSize = NSSize(width: 600, height: 450)
            window.isReleasedWhenClosed = false
            window.delegate = self
            dashboardWindow = window
        }

        window.contentView = NSHostingView(rootView: contentView)
        NSApp.setActivationPolicy(.regular)  // <-- NEW: show dock icon
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
```

- [ ] **Step 2: Add `.accessory` policy when closing dashboard**

In `windowShouldClose(_:)`, add one line after `sender.orderOut(nil)`:

```swift
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard sender === dashboardWindow else { return true }
        while let sheet = sender.attachedSheet {
            sender.endSheet(sheet)
        }
        currentViewModel?.navigation = .dashboard
        sender.contentView = nil
        sender.orderOut(nil)
        NSApp.setActivationPolicy(.accessory)  // <-- NEW: hide dock icon
        return false
    }
```

- [ ] **Step 3: Add `applicationShouldHandleReopen` for dock icon click**

Add this new method to `AppDelegate`, after `windowShouldClose`:

```swift
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if let window = dashboardWindow, window.isVisible {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
        }
        return false
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
git add ClaudeDashboard/ClaudeDashboardApp.swift
git commit -m "feat: dynamic dock icon — show when dashboard is open, hide when closed"
```

---

### Task 2: First Launch Auto-Open Dashboard

**Files:**
- Modify: `ClaudeDashboard/ClaudeDashboardApp.swift`

- [ ] **Step 1: Add first-launch check in MenuBarExtra `.onAppear`**

Add an `.onAppear` modifier to the `MenuBarPopover` inside the `MenuBarExtra` content closure. This fires once when the menu bar extra is first rendered (app launch):

```swift
    var body: some Scene {
        MenuBarExtra {
            MenuBarPopover(viewModel: viewModel) {
                appDelegate.openDashboardWindow(viewModel: viewModel)
            }
            .onAppear {
                let key = "claude-dashboard.hasLaunchedBefore"
                if !UserDefaults.standard.bool(forKey: key) {
                    UserDefaults.standard.set(true, forKey: key)
                    appDelegate.openDashboardWindow(viewModel: viewModel)
                }
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "chart.bar.fill")
                Text(viewModel.menuBarLabel)
                    .monospacedDigit()
            }
        }
        .menuBarExtraStyle(.window)
    }
```

- [ ] **Step 2: Build and verify**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `BUILD SUCCEEDED`

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/ClaudeDashboardApp.swift
git commit -m "feat: auto-open dashboard on first app launch"
```

---

### Task 3: MenuBarPopover Empty State

**Files:**
- Modify: `ClaudeDashboard/Views/MenuBarPopover.swift`

- [ ] **Step 1: Hide refresh button when no accounts**

Wrap the refresh button (lines 16-26) in a conditional. The full header `HStack` becomes:

```swift
            HStack {
                Text("Claude Dashboard")
                    .font(.headline)

                Spacer()

                if !viewModel.accountStates.isEmpty {
                    Button(action: {
                        Task { await viewModel.refreshAll() }
                    }) {
                        Image(systemName: "arrow.clockwise")
                            .font(.caption)
                            .frame(width: 24, height: 24)
                            .background(.quaternary)
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                    }
                    .buttonStyle(.borderless)
                    .disabled(viewModel.isRefreshing)
                }

                Button(action: {
                    let popover = NSApp.keyWindow
                    onOpenWindow()
                    popover?.close()
                }) {
                    Image(systemName: "rectangle.expand.vertical")
                        .font(.caption)
                        .frame(width: 24, height: 24)
                        .background(.quaternary)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                }
                .buttonStyle(.borderless)
            }
```

- [ ] **Step 2: Add "Add Account" button to empty state**

Replace the `emptyState` computed property with an actionable version:

```swift
    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.largeTitle)
                .foregroundStyle(.secondary)

            Text("No accounts configured")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Button(action: {
                let popover = NSApp.keyWindow
                onOpenWindow()
                popover?.close()
            }) {
                Text("Add Account")
                    .font(.subheadline.weight(.medium))
            }
            .buttonStyle(.borderedProminent)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }
```

- [ ] **Step 3: Build and verify**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `BUILD SUCCEEDED`

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/Views/MenuBarPopover.swift
git commit -m "feat: popover empty state — hide refresh, add 'Add Account' button"
```

---

### Task 4: DashboardWindow Empty State

**Files:**
- Modify: `ClaudeDashboard/Views/DashboardWindow.swift`

- [ ] **Step 1: Add `showingSetup` state and sheet**

Add a new `@State` property next to `showingSettings`, and a second `.sheet` modifier to the `Group` in `body`:

```swift
struct DashboardWindow: View {
    @ObservedObject var viewModel: DashboardViewModel
    @State private var showingSettings = false
    @State private var showingSetup = false    // <-- NEW

    var body: some View {
        Group {
            switch viewModel.navigation {
            case .dashboard:
                dashboardContent
            case .accountDetail(let accountId):
                if let state = viewModel.accountStates.first(where: { $0.id == accountId }) {
                    AccountDetailView(
                        viewModel: AccountDetailViewModel(
                            accountId: accountId,
                            accountName: state.account.name,
                            accountPlan: state.account.plan,
                            logStore: viewModel.logStore
                        ),
                        onBack: { viewModel.navigation = .dashboard }
                    )
                }
            case .overview:
                OverviewChartView(
                    viewModel: viewModel,
                    onBack: { viewModel.navigation = .dashboard }
                )
            }
        }
        .frame(minWidth: 600, minHeight: 450)
        .sheet(isPresented: $showingSettings) {
            SettingsView(viewModel: viewModel)
        }
        .sheet(isPresented: $showingSetup) {               // <-- NEW
            SetupView(viewModel: viewModel) {               // <-- NEW
                showingSetup = false                         // <-- NEW
            }                                               // <-- NEW
        }                                                   // <-- NEW
    }
```

- [ ] **Step 2: Hide Refresh and Overview buttons when no accounts**

Wrap the Overview and Refresh buttons in a conditional inside `dashboardContent`:

```swift
    private var dashboardContent: some View {
        VStack(spacing: 0) {
            // Toolbar
            HStack {
                Text("Claude Dashboard")
                    .font(.title2.bold())

                Spacer()

                if !viewModel.accountStates.isEmpty {
                    Button(action: { viewModel.navigation = .overview }) {
                        Label("Overview", systemImage: "chart.xyaxis.line")
                    }

                    Button(action: {
                        Task { await viewModel.refreshAll() }
                    }) {
                        Label("Refresh", systemImage: "arrow.clockwise")
                    }
                    .disabled(viewModel.isRefreshing)
                }

                Button(action: { showingSettings = true }) {
                    Label("Settings", systemImage: "gearshape")
                }
            }
            .padding()

            Divider()

            // Cards grid
            if viewModel.accountStates.isEmpty {
                emptyStateView
            } else {
                ScrollView {
                    LazyVStack(spacing: 12) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state, onResync: {
                                Task { await viewModel.resyncAccount(state.id) }
                            }, onTogglePin: {
                                viewModel.togglePin(for: state.id)
                            }, onTap: {
                                viewModel.navigation = .accountDetail(state.id)
                            })
                        }
                    }
                    .padding()
                }
            }
        }
    }
```

- [ ] **Step 3: Add "Add Account" button to empty state**

Replace `emptyStateView` with an actionable version:

```swift
    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Spacer()
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No Accounts")
                .font(.title3.bold())
            Text("Sync your Claude accounts from Chrome to get started.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Button(action: { showingSetup = true }) {
                Text("Add Account")
                    .font(.body.weight(.medium))
            }
            .buttonStyle(.borderedProminent)
            Spacer()
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
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
git add ClaudeDashboard/Views/DashboardWindow.swift
git commit -m "feat: dashboard empty state — hide refresh/overview, add 'Add Account' button"
```

---

### Task 5: Run Full Test Suite

- [ ] **Step 1: Run all tests**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | tail -20
```
Expected: `TEST SUCCEEDED` with all tests passing. These changes are UI-only and should not break any existing service/model tests.

- [ ] **Step 2: Manual verification checklist**

Launch the app and verify:
1. First launch: dashboard opens automatically (delete `claude-dashboard.hasLaunchedBefore` from UserDefaults to simulate: `defaults delete com.haicao.ClaudeDashboard claude-dashboard.hasLaunchedBefore`)
2. Dock icon appears when dashboard is open
3. Dock icon disappears when dashboard is closed (red X)
4. Clicking dock icon brings dashboard to foreground
5. Popover: refresh button hidden when no accounts
6. Popover: "Add Account" button visible, opens dashboard + SetupView
7. Dashboard: Refresh/Overview buttons hidden when no accounts
8. Dashboard: "Add Account" button visible, opens SetupView sheet
9. Dashboard: Settings button always visible
10. After adding an account: refresh buttons reappear, empty state disappears
