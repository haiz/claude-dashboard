# UX Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve dashboard UX with widget dismiss-on-expand, reset timer urgency coloring, Max 5x/20x plan detection, smart account ordering by consumption rate, and Sonnet-only limit display.

**Architecture:** Extend existing data models to parse additional API fields (`seven_day_sonnet`, plan tier from orgs). Add urgency color logic to `UsageBar`. Sort `accountStates` by burn rate in `DashboardViewModel`. Pass dismiss closure through `MenuBarPopover`.

**Tech Stack:** SwiftUI, Foundation, XCTest

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `ClaudeDashboard/Models/Account.swift` | Modify | Add `max5x`, `max20x` plan cases |
| `ClaudeDashboard/Models/UsageData.swift` | Modify | Add `sevenDaySonnet` optional field |
| `ClaudeDashboard/Services/UsageAPIService.swift` | Modify | Parse `seven_day_sonnet`, detect plan tier from orgs |
| `ClaudeDashboard/Views/UsageBar.swift` | Modify | Add urgency color to reset timer text |
| `ClaudeDashboard/Views/AccountCard.swift` | Modify | Show Sonnet usage bar, update plan badge for 5x/20x |
| `ClaudeDashboard/ViewModels/DashboardViewModel.swift` | Modify | Smart ordering by consumption rate |
| `ClaudeDashboard/Views/MenuBarPopover.swift` | Modify | Add `onDismiss` closure, call on expand |
| `ClaudeDashboard/ClaudeDashboardApp.swift` | Modify | Wire dismiss closure to popover |
| `ClaudeDashboardTests/UsageDataTests.swift` | Modify | Test Sonnet field parsing |
| `ClaudeDashboardTests/UsageAPIServiceTests.swift` | Modify | Test plan detection, Sonnet parsing |

---

### Task 1: Update AccountPlan enum for Max 5x / 20x

**Files:**
- Modify: `ClaudeDashboard/Models/Account.swift:1-6`

- [ ] **Step 1: Update AccountPlan enum**

```swift
enum AccountPlan: String, Codable, CaseIterable {
    case pro = "Pro"
    case max5x = "Max 5x"
    case max20x = "Max 20x"
    case max200 = "Max"  // fallback when tier unknown
}
```

- [ ] **Step 2: Build to verify no compilation errors**

Run: `xcodebuild -scheme ClaudeDashboard -destination 'platform=macOS' build 2>&1 | tail -5`
Expected: BUILD SUCCEEDED (may have warnings about exhaustive switch — we fix those in later tasks)

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Models/Account.swift
git commit -m "feat: add Max 5x/20x plan cases to AccountPlan enum"
```

---

### Task 2: Add Sonnet limit to UsageData

**Files:**
- Modify: `ClaudeDashboard/Models/UsageData.swift:13-20`

- [ ] **Step 1: Write failing test for Sonnet field parsing**

Add to `ClaudeDashboardTests/UsageDataTests.swift`:

```swift
func testDecodesSevenDaySonnet() throws {
    let json = """
    {
      "five_hour": { "utilization": 42.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
      "seven_day": { "utilization": 18.0, "resets_at": "2026-04-14T16:59:59.661657+00:00" },
      "seven_day_sonnet": { "utilization": 25.0, "resets_at": "2026-04-12T10:00:00+00:00" }
    }
    """.data(using: .utf8)!

    let usage = try UsageData.decode(from: json)

    XCTAssertNotNil(usage.sevenDaySonnet)
    XCTAssertEqual(usage.sevenDaySonnet?.utilization, 25.0)
    XCTAssertNotNil(usage.sevenDaySonnet?.resetsAt)
}

func testDecodesNullSevenDaySonnet() throws {
    let json = """
    {
      "five_hour": { "utilization": 0.0, "resets_at": null },
      "seven_day": { "utilization": 0.0, "resets_at": null },
      "seven_day_sonnet": null
    }
    """.data(using: .utf8)!

    let usage = try UsageData.decode(from: json)

    XCTAssertNil(usage.sevenDaySonnet)
}

func testDecodesMissingSonnetField() throws {
    let json = """
    {
      "five_hour": { "utilization": 10.0, "resets_at": null },
      "seven_day": { "utilization": 5.0, "resets_at": null }
    }
    """.data(using: .utf8)!

    let usage = try UsageData.decode(from: json)

    XCTAssertNil(usage.sevenDaySonnet)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `xcodebuild test -scheme ClaudeDashboard -destination 'platform=macOS' -only-testing:ClaudeDashboardTests/UsageDataTests 2>&1 | grep -E '(Test Case|FAIL|PASS|error:)'`
Expected: FAIL — `sevenDaySonnet` does not exist on `UsageData`

- [ ] **Step 3: Add sevenDaySonnet to UsageData**

Replace the `UsageData` struct in `ClaudeDashboard/Models/UsageData.swift`:

```swift
struct UsageData: Codable, Equatable {
    let fiveHour: UsageLimit
    let sevenDay: UsageLimit
    let sevenDaySonnet: UsageLimit?

    enum CodingKeys: String, CodingKey {
        case fiveHour = "five_hour"
        case sevenDay = "seven_day"
        case sevenDaySonnet = "seven_day_sonnet"
    }

    static func decode(from data: Data) throws -> UsageData {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let container = try decoder.singleValueContainer()
            let dateString = try container.decode(String.self)

            let formatterWithFraction = ISO8601DateFormatter()
            formatterWithFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            if let date = formatterWithFraction.date(from: dateString) {
                return date
            }

            let formatterBasic = ISO8601DateFormatter()
            formatterBasic.formatOptions = [.withInternetDateTime]
            if let date = formatterBasic.date(from: dateString) {
                return date
            }

            throw DecodingError.dataCorruptedError(
                in: container,
                debugDescription: "Cannot decode date: \(dateString)"
            )
        }
        return try decoder.decode(UsageData.self, from: data)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `xcodebuild test -scheme ClaudeDashboard -destination 'platform=macOS' -only-testing:ClaudeDashboardTests/UsageDataTests 2>&1 | grep -E '(Test Case|FAIL|PASS)'`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add ClaudeDashboard/Models/UsageData.swift ClaudeDashboardTests/UsageDataTests.swift
git commit -m "feat: parse seven_day_sonnet from usage API response"
```

---

### Task 3: Detect Max 5x/20x from organizations API

**Files:**
- Modify: `ClaudeDashboard/Services/UsageAPIService.swift:46-67` (fetchOrganizations)
- Modify: `ClaudeDashboard/Services/UsageAPIService.swift:71-95` (fetchFullUsage)

The organizations endpoint returns a `capabilities` array and potentially other fields. We parse known patterns for plan tier detection. Also check `active_flags` and `settings` fields in the raw JSON.

- [ ] **Step 1: Update OrgInfo to include planHint**

In `UsageAPIService.swift`, update the `OrgInfo` struct:

```swift
struct OrgInfo {
    let uuid: String
    let name: String
    let email: String?
    let capabilities: [String]
    let planHint: AccountPlan?
}
```

- [ ] **Step 2: Parse plan tier from organizations response**

Update `fetchOrganizations` to detect plan tier from the raw JSON. Look in `capabilities`, `active_flags`, `settings`, and `billing_info` for tier clues:

```swift
func fetchOrganizations(sessionKey: String) async throws -> [OrgInfo] {
    let url = URL(string: "\(baseURL)/organizations")!
    let request = makeRequest(url: url, sessionKey: sessionKey)

    let (data, response) = try await session.data(for: request)
    let _ = try validateResponse(response)

    guard let jsonArray = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
        return []
    }

    return jsonArray.compactMap { dict in
        guard let uuid = dict["uuid"] as? String,
              let name = dict["name"] as? String else {
            return nil
        }
        let capabilities = dict["capabilities"] as? [String] ?? []
        let email = dict["email_address"] as? String
            ?? (dict["billing_info"] as? [String: Any])?["email"] as? String

        let planHint = Self.detectPlanTier(from: dict, capabilities: capabilities)
        return OrgInfo(uuid: uuid, name: name, email: email, capabilities: capabilities, planHint: planHint)
    }
}

private static func detectPlanTier(from dict: [String: Any], capabilities: [String]) -> AccountPlan? {
    // Check all string fields in the JSON for plan tier patterns
    let jsonString = (try? JSONSerialization.data(withJSONObject: dict))
        .flatMap { String(data: $0, encoding: .utf8) }?.lowercased() ?? ""

    // Look for explicit 5x/20x markers in the entire org JSON
    if jsonString.contains("max_20x") || jsonString.contains("max20x") {
        return .max20x
    }
    if jsonString.contains("max_5x") || jsonString.contains("max5x") {
        return .max5x
    }

    // Check capabilities for known patterns
    let capsJoined = capabilities.joined(separator: " ").lowercased()
    if capsJoined.contains("max") || capsJoined.contains("extra_usage") {
        return .max200  // Max, but can't determine 5x/20x
    }

    return nil
}
```

- [ ] **Step 3: Update fetchFullUsage to also check extra_usage for tier**

In `fetchFullUsage`, enhance the plan detection to look for tier info in `extra_usage`:

```swift
func fetchFullUsage(orgId: String, sessionKey: String) async throws -> (usage: UsageData, planHint: AccountPlan?, newSessionKey: String?) {
    let url = URL(string: "\(baseURL)/organizations/\(orgId)/usage")!
    let request = makeRequest(url: url, sessionKey: sessionKey)

    let (data, response) = try await session.data(for: request)
    let httpResponse = try validateResponse(response)

    let usage = try UsageData.decode(from: data)
    let newSessionKey = parseSessionKey(from: httpResponse)

    // Detect plan from raw JSON
    var planHint: AccountPlan? = nil
    if let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
        if let extraUsage = json["extra_usage"] as? [String: Any],
           let isEnabled = extraUsage["is_enabled"] as? Bool,
           isEnabled {
            // Check for tier/multiplier in extra_usage
            if let tier = extraUsage["tier"] as? String {
                if tier.contains("20x") { planHint = .max20x }
                else if tier.contains("5x") { planHint = .max5x }
                else { planHint = .max200 }
            } else if let multiplier = extraUsage["multiplier"] as? Int {
                if multiplier >= 20 { planHint = .max20x }
                else if multiplier >= 5 { planHint = .max5x }
                else { planHint = .max200 }
            } else {
                planHint = .max200
            }
        } else {
            planHint = .pro
        }
    }

    return (usage: usage, planHint: planHint, newSessionKey: newSessionKey)
}
```

- [ ] **Step 4: Build to verify compilation**

Run: `xcodebuild -scheme ClaudeDashboard -destination 'platform=macOS' build 2>&1 | tail -5`
Expected: BUILD SUCCEEDED

- [ ] **Step 5: Commit**

```bash
git add ClaudeDashboard/Services/UsageAPIService.swift
git commit -m "feat: detect Max 5x/20x plan tier from organizations and usage API"
```

---

### Task 4: Add reset timer urgency coloring to UsageBar

**Files:**
- Modify: `ClaudeDashboard/Views/UsageBar.swift`

The urgency color logic: `timeRemaining / totalLimitWindow` ratio. Low ratio (near reset) = green. High ratio (long wait) = red. Same HSB interpolation as usage color but based on time fraction.

- [ ] **Step 1: Add urgency color and totalSeconds parameter to UsageBar**

Replace the entire `UsageBar.swift`:

```swift
import SwiftUI

struct UsageBar: View {
    let label: String           // "5h", "7d", or "S"
    let utilization: Double     // 0-100
    let resetsAt: Date?
    let totalSeconds: TimeInterval  // total window: 18000 for 5h, 604800 for 7d

    init(label: String, utilization: Double, resetsAt: Date?, totalSeconds: TimeInterval = 18000) {
        self.label = label
        self.utilization = utilization
        self.resetsAt = resetsAt
        self.totalSeconds = totalSeconds
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(width: 24, alignment: .leading)

                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.primary.opacity(0.1))

                        RoundedRectangle(cornerRadius: 4)
                            .fill(DashboardViewModel.usageColor(for: utilization))
                            .frame(width: geo.size.width * min(utilization / 100, 1.0))
                    }
                }
                .frame(height: 8)

                Text("\(Int(utilization))%")
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 36, alignment: .trailing)
            }

            if let resetsAt {
                Text("resets in \(formatTimeRemaining(resetsAt))")
                    .font(.caption2)
                    .foregroundStyle(resetUrgencyColor(resetsAt))
                    .padding(.leading, 28)
            }
        }
    }

    /// Color based on how close the reset is relative to total window.
    /// Near reset (low ratio) = green. Far from reset (high ratio) = muted/tertiary.
    private func resetUrgencyColor(_ date: Date) -> Color {
        let remaining = date.timeIntervalSinceNow
        guard remaining > 0, totalSeconds > 0 else {
            return .green
        }

        let fraction = min(remaining / totalSeconds, 1.0)

        // fraction 0 = about to reset (green), fraction 1 = just started (tertiary gray)
        // Below 30% remaining time: interpolate green intensity
        // Above 30%: show as tertiary (default muted)
        if fraction > 0.3 {
            return .secondary.opacity(0.6)
        }

        // 0.0 → bright green, 0.3 → dim green/gray
        let greenIntensity = 1.0 - (fraction / 0.3)
        return Color(hue: 120.0 / 360.0, saturation: 0.6 * greenIntensity + 0.1, brightness: 0.5 + 0.35 * greenIntensity)
    }

    private func formatTimeRemaining(_ date: Date) -> String {
        let remaining = date.timeIntervalSinceNow
        guard remaining > 0 else { return "now" }

        let days = Int(remaining) / 86400
        let hours = (Int(remaining) % 86400) / 3600
        let minutes = (Int(remaining) % 3600) / 60

        if days > 0 {
            return "\(days)d \(hours)h"
        } else if hours > 0 {
            return "\(hours)h \(String(format: "%02d", minutes))m"
        } else {
            return "\(minutes)m"
        }
    }
}
```

- [ ] **Step 2: Update AccountCard to pass totalSeconds for each bar**

In `AccountCard.swift`, update the `usageContent` function:

```swift
private func usageContent(_ usage: UsageData) -> some View {
    VStack(spacing: 8) {
        UsageBar(label: "5h", utilization: usage.fiveHour.utilization, resetsAt: usage.fiveHour.resetsAt, totalSeconds: 18000)
        UsageBar(label: "7d", utilization: usage.sevenDay.utilization, resetsAt: usage.sevenDay.resetsAt, totalSeconds: 604800)
        if let sonnet = usage.sevenDaySonnet {
            UsageBar(label: "S", utilization: sonnet.utilization, resetsAt: sonnet.resetsAt, totalSeconds: 604800)
        }
    }
}
```

- [ ] **Step 3: Build to verify**

Run: `xcodebuild -scheme ClaudeDashboard -destination 'platform=macOS' build 2>&1 | tail -5`
Expected: BUILD SUCCEEDED

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/Views/UsageBar.swift ClaudeDashboard/Views/AccountCard.swift
git commit -m "feat: add reset timer urgency coloring and Sonnet usage bar"
```

---

### Task 5: Update AccountCard plan badge for 5x/20x

**Files:**
- Modify: `ClaudeDashboard/Views/AccountCard.swift:25-30`

- [ ] **Step 1: Update plan badge to handle all cases**

Replace the plan badge section in `AccountCard.swift`:

```swift
// Plan badge
Text(state.account.plan.rawValue)
    .font(.caption2.bold())
    .padding(.horizontal, 6)
    .padding(.vertical, 2)
    .background(planBadgeColor.opacity(0.15))
    .clipShape(Capsule())
```

Add a computed property to `AccountCard`:

```swift
private var planBadgeColor: Color {
    switch state.account.plan {
    case .pro: return .blue
    case .max5x: return .purple
    case .max20x: return .orange
    case .max200: return .purple
    }
}
```

- [ ] **Step 2: Build to verify**

Run: `xcodebuild -scheme ClaudeDashboard -destination 'platform=macOS' build 2>&1 | tail -5`
Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/AccountCard.swift
git commit -m "feat: color-coded plan badges for Pro, Max 5x, Max 20x"
```

---

### Task 6: Smart ordering by consumption rate

**Files:**
- Modify: `ClaudeDashboard/ViewModels/DashboardViewModel.swift`

Sort accounts by `utilization / max(timeToReset, 60)` — higher burn rate first. Expired/error accounts go to bottom.

- [ ] **Step 1: Add sorting to accountStates**

Add a `sortedByBurnRate` computed property and use it in `syncStates`:

In `DashboardViewModel.swift`, add after the `usageColor` static method:

```swift
/// Burn rate = utilization / time remaining. Higher = consuming faster = shown first.
static func burnRate(for state: AccountUsageState) -> Double {
    guard state.account.status == .active,
          let usage = state.usage else {
        return -1  // expired/error/no-data go to bottom
    }

    let utilization = usage.fiveHour.utilization
    let timeRemaining: TimeInterval
    if let resetsAt = usage.fiveHour.resetsAt {
        timeRemaining = max(resetsAt.timeIntervalSinceNow, 60)
    } else {
        timeRemaining = 18000  // assume full 5h if no reset time
    }

    return utilization / timeRemaining
}
```

- [ ] **Step 2: Apply sorting after refresh and sync**

Update `refreshAll()` — add sorting at the end (before the closing brace of `refreshAll`):

```swift
// Sort by burn rate after refresh
accountStates.sort { DashboardViewModel.burnRate(for: $0) > DashboardViewModel.burnRate(for: $1) }
```

Update `syncStates(with:)` — add sorting at the end:

```swift
private func syncStates(with accounts: [Account]) {
    let existingMap = Dictionary(uniqueKeysWithValues: accountStates.map { ($0.id, $0) })
    accountStates = accounts.map { account in
        if let existing = existingMap[account.id] {
            return AccountUsageState(
                id: account.id,
                account: account,
                usage: existing.usage,
                isLoading: existing.isLoading,
                error: existing.error
            )
        }
        return AccountUsageState(id: account.id, account: account)
    }
    // Sort by burn rate (accounts with data sort above those without)
    accountStates.sort { DashboardViewModel.burnRate(for: $0) > DashboardViewModel.burnRate(for: $1) }
}
```

- [ ] **Step 3: Build to verify**

Run: `xcodebuild -scheme ClaudeDashboard -destination 'platform=macOS' build 2>&1 | tail -5`
Expected: BUILD SUCCEEDED

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/ViewModels/DashboardViewModel.swift
git commit -m "feat: sort accounts by token consumption burn rate"
```

---

### Task 7: Close widget on expand

**Files:**
- Modify: `ClaudeDashboard/Views/MenuBarPopover.swift:3-5`
- Modify: `ClaudeDashboard/ClaudeDashboardApp.swift:10-13`

- [ ] **Step 1: Add onDismiss to MenuBarPopover**

Update `MenuBarPopover`:

```swift
struct MenuBarPopover: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onOpenWindow: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("Claude Dashboard")
                    .font(.headline)

                Spacer()

                Button(action: {
                    Task { await viewModel.refreshAll() }
                }) {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.borderless)
                .disabled(viewModel.isRefreshing)

                Button(action: {
                    onOpenWindow()
                    dismiss()
                }) {
                    Image(systemName: "rectangle.expand.vertical")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)

            Divider()

            // Account cards
            if viewModel.accountStates.isEmpty {
                emptyState
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state) {
                                viewModel.resyncAccount(state.id)
                            }
                        }
                    }
                    .padding(12)
                }
            }
        }
        .frame(width: 320)
        .frame(maxHeight: 500)
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.largeTitle)
                .foregroundStyle(.secondary)

            Text("No accounts configured")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Text("Open Settings to sync from Chrome")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }
}
```

Note: `@Environment(\.dismiss)` works for MenuBarExtra popover windows in SwiftUI. If it doesn't dismiss the popover (MenuBarExtra uses NSPanel internally), we use an alternative approach with `NSApp.keyWindow?.close()` instead:

```swift
Button(action: {
    onOpenWindow()
    // Dismiss the popover by closing its key window
    DispatchQueue.main.async {
        NSApp.keyWindow?.close()
    }
}) {
    Image(systemName: "rectangle.expand.vertical")
}
```

Use the `NSApp.keyWindow?.close()` approach if `dismiss()` doesn't work at runtime.

- [ ] **Step 2: Build and manually test**

Run: `xcodebuild -scheme ClaudeDashboard -destination 'platform=macOS' build 2>&1 | tail -5`
Expected: BUILD SUCCEEDED

Manual test: Click menu bar icon → click expand → popover should close and dashboard window should open.

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/MenuBarPopover.swift
git commit -m "feat: close widget popover when expand button is clicked"
```

---

### Task 8: Wire plan detection into refresh flow

**Files:**
- Modify: `ClaudeDashboard/ViewModels/DashboardViewModel.swift:36-80`

Currently `refreshAll` calls `fetchUsage` which doesn't return plan info. Update to use `fetchFullUsage` so plan tier is detected on each refresh.

- [ ] **Step 1: Update refreshAll to use fetchFullUsage**

Replace the task group body in `refreshAll`:

```swift
group.addTask { [apiService, accountStore] in
    do {
        let (usage, planHint, newKey) = try await apiService.fetchFullUsage(orgId: orgId, sessionKey: sessionKey)
        if let newKey {
            accountStore.saveSessionKey(newKey, for: account.id)
        }
        return (account.id, usage, nil, planHint)
    } catch UsageAPIError.authExpired {
        return (account.id, nil, "expired", nil)
    } catch {
        return (account.id, nil, error.localizedDescription, nil)
    }
}
```

Update the result type to include `AccountPlan?`:

```swift
await withTaskGroup(of: (UUID, UsageData?, String?, AccountPlan?).self) { group in
```

Update the result handling loop:

```swift
for await (accountId, usage, error, planHint) in group {
    if let index = accountStates.firstIndex(where: { $0.id == accountId }) {
        accountStates[index].usage = usage ?? accountStates[index].usage
        accountStates[index].error = error

        if error == "expired" {
            var account = accountStates[index].account
            account.status = .expired
            accountStore.updateAccount(account)
        } else if error == nil {
            var account = accountStates[index].account
            account.status = .active
            account.lastSynced = Date()
            if let planHint, account.plan != planHint {
                account.plan = planHint
            }
            accountStore.updateAccount(account)
        }
    }
}
```

- [ ] **Step 2: Add sorting call at end of refreshAll**

After the task group completes (after the closing `}` of `withTaskGroup`), before `defer { isRefreshing = false }` triggers:

```swift
accountStates.sort { DashboardViewModel.burnRate(for: $0) > DashboardViewModel.burnRate(for: $1) }
```

- [ ] **Step 3: Build to verify**

Run: `xcodebuild -scheme ClaudeDashboard -destination 'platform=macOS' build 2>&1 | tail -5`
Expected: BUILD SUCCEEDED

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/ViewModels/DashboardViewModel.swift
git commit -m "feat: wire plan tier detection and sorting into refresh flow"
```

---

### Task 9: Update existing tests for new model fields

**Files:**
- Modify: `ClaudeDashboardTests/AccountStoreTests.swift`
- Modify: `ClaudeDashboardTests/UsageAPIServiceTests.swift`

- [ ] **Step 1: Fix any tests using old AccountPlan values**

Search for `.max200` in tests and update if needed. The existing test JSON responses for `fetchUsage` don't include `seven_day_sonnet`, but that's OK since it's optional.

Update any test that creates `Account` objects to use valid plan values. Check `AccountStoreTests.swift` for account creation.

- [ ] **Step 2: Add test for fetchFullUsage plan detection**

Add to `UsageAPIServiceTests.swift`:

```swift
func testFetchFullUsageDetectsMaxPlan() async throws {
    let responseJSON = """
    {
      "five_hour": { "utilization": 30.0, "resets_at": null },
      "seven_day": { "utilization": 10.0, "resets_at": null },
      "seven_day_sonnet": { "utilization": 5.0, "resets_at": null },
      "extra_usage": { "is_enabled": true }
    }
    """.data(using: .utf8)!

    MockURLProtocol.requestHandler = { request in
        let response = HTTPURLResponse(
            url: request.url!, statusCode: 200,
            httpVersion: nil, headerFields: nil
        )!
        return (response, responseJSON)
    }

    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [MockURLProtocol.self]
    let session = URLSession(configuration: config)

    let service = UsageAPIService(session: session)
    let (usage, planHint, _) = try await service.fetchFullUsage(orgId: "org-123", sessionKey: "sk-test")

    XCTAssertEqual(usage.fiveHour.utilization, 30.0)
    XCTAssertEqual(usage.sevenDaySonnet?.utilization, 5.0)
    XCTAssertEqual(planHint, .max200)  // extra_usage enabled but no tier info
}

func testFetchFullUsageDetectsProPlan() async throws {
    let responseJSON = """
    {
      "five_hour": { "utilization": 10.0, "resets_at": null },
      "seven_day": { "utilization": 5.0, "resets_at": null },
      "extra_usage": null
    }
    """.data(using: .utf8)!

    MockURLProtocol.requestHandler = { request in
        let response = HTTPURLResponse(
            url: request.url!, statusCode: 200,
            httpVersion: nil, headerFields: nil
        )!
        return (response, responseJSON)
    }

    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [MockURLProtocol.self]
    let session = URLSession(configuration: config)

    let service = UsageAPIService(session: session)
    let (_, planHint, _) = try await service.fetchFullUsage(orgId: "org-123", sessionKey: "sk-test")

    XCTAssertEqual(planHint, .pro)
}
```

- [ ] **Step 3: Run all tests**

Run: `xcodebuild test -scheme ClaudeDashboard -destination 'platform=macOS' 2>&1 | grep -E '(Test Case|FAIL|PASS|Executed)'`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboardTests/
git commit -m "test: add tests for plan detection and Sonnet limit parsing"
```

---

### Task 10: Final verification

- [ ] **Step 1: Full build**

Run: `xcodebuild -scheme ClaudeDashboard -destination 'platform=macOS' build 2>&1 | tail -5`
Expected: BUILD SUCCEEDED

- [ ] **Step 2: Full test suite**

Run: `xcodebuild test -scheme ClaudeDashboard -destination 'platform=macOS' 2>&1 | grep -E '(Test Case|FAIL|PASS|Executed)'`
Expected: All tests pass, 0 failures

- [ ] **Step 3: Manual smoke test**

Launch the app and verify:
1. Widget popover closes when expand button is clicked
2. Reset timer text shows green color when near reset time
3. Plan badge shows "Max 5x" / "Max 20x" / "Max" / "Pro" correctly
4. Accounts are ordered by burn rate (highest consumption first)
5. Sonnet-only bar (labeled "S") appears when data is available

- [ ] **Step 4: Final commit if any fixes needed**
