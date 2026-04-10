# Burn Rate Animal Icons + Usage Logging Design

## Overview

Two interconnected features for Claude Dashboard:
1. **Burn Rate Calculation + Animal Icons** — Calculate token consumption speed per account, display animal emoji on progress bars indicating speed level.
2. **Usage Logging + Charts** — Persist usage data to SQLite, provide drill-down charts per account and an aggregated overview chart.

## Architecture: Approach B (Separated Layer)

New files:
- `Services/UsageLogStore.swift` — SQLite CRUD + smart compression
- `Services/BurnRateTracker.swift` — Speed calculation, animal selection
- `ViewModels/AccountDetailViewModel.swift` — Query logs for charts
- `Views/AccountDetailView.swift` — Drill-down chart per account
- `Views/OverviewChartView.swift` — Aggregated multi-account chart

Modified files:
- `Views/UsageBar.swift` — Animal overlay on progress bar
- `Views/AccountCard.swift` — Pass animal, click handler for drill-down
- `ViewModels/DashboardViewModel.swift` — Inject tracker, call record, navigation state
- `Views/DashboardWindow.swift` — Overview button, navigation

---

## Section 1: UsageLogStore (SQLite)

### Schema

```sql
CREATE TABLE accounts_map (
    aid INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL UNIQUE
);

CREATE TABLE usage_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    aid INTEGER NOT NULL,           -- FK → accounts_map
    w INTEGER NOT NULL,             -- 0=5h, 1=7d, 2=sonnet
    rat INTEGER NOT NULL,           -- resets_at: unix timestamp
    t INTEGER NOT NULL,             -- recorded_at: unix timestamp
    u INTEGER NOT NULL,             -- utilization × 100 (0-10000)
    lim INTEGER DEFAULT 0           -- is_limited: 0 or 1
);

CREATE INDEX idx_logs_lookup ON usage_logs(aid, w, rat, t);
```

Window enum mapping:
```swift
enum UsageWindow: Int {
    case fiveHour = 0
    case sevenDay = 1
    case sonnet = 2
}
```

### Smart Compression (Insert Logic)

When inserting a new data point for `(aid, w, rat)`:

1. Fetch the 2 most recent records for the same `(aid, w, rat)`.
2. If < 2 records exist → INSERT normally.
3. If the 2 most recent records have the same `u` AND the new record also has the same `u`:
   - DELETE the most recent record (the "middle" one), then INSERT the new record.
   - Result: keeps first point + latest point of an unchanged streak.
4. If `u` differs → INSERT normally.

This preserves exact start/end timestamps of every plateau while minimizing row count.

### Data Types

```swift
struct UsageLogEntry {
    let accountId: UUID
    let window: UsageWindow
    let resetsAt: Date
    let recordedAt: Date
    let utilization: Double    // 0-100 (decoded from 0-10000 int)
    let isLimited: Bool
}

struct ResetCycle {
    let resetsAt: Date
    let firstRecordedAt: Date
    let lastRecordedAt: Date
    let peakUtilization: Double
    let dataPointCount: Int
}
```

### API

```swift
actor UsageLogStore {
    func record(accountId: UUID, window: UsageWindow, resetsAt: Date,
                utilization: Double, isLimited: Bool)
    func logs(accountId: UUID, window: UsageWindow,
              from: Date?, to: Date?) -> [UsageLogEntry]
    func allLogs(window: UsageWindow,
                 from: Date?, to: Date?) -> [UsageLogEntry]  // returns all accounts, distinguished by accountId
    func resetCycles(accountId: UUID, window: UsageWindow) -> [ResetCycle]
    func deleteOlderThan(_ date: Date)
}
```

DB location: `~/Library/Application Support/ClaudeDashboard/usage_logs.db`

Cleanup: `deleteOlderThan()` runs on each app launch, removes logs > 90 days.

---

## Section 2: BurnRateTracker

### Responsibility

Receives usage data on each refresh → calculates consumption speed → returns animal emoji + level. Also calls UsageLogStore to persist logs.

### Internal State (in-memory)

```swift
struct Measurement {
    let utilization: Double
    let recordedAt: Date
    let resetsAt: Date
}

// Per (accountId, window), keep up to 2 most recent measurements
var history: [String: (prev: Measurement?, current: Measurement?)]
// key = "\(accountId)_\(window.rawValue)"
```

### Speed Calculation Logic

When a new measurement arrives:

1. **Check same reset cycle** (`resetsAt == current.resetsAt`)?
   - No → reset history for this key, store new measurement, return nil.

2. **Compare utilization with current:**
   - **Changed** → calculate speed:
     - `delta% = new.utilization - current.utilization`
     - `deltaTime = new.recordedAt - current.recordedAt`
     - `rate = delta% / deltaTime` (% per second)
     - `projectedTime = (100 - new.utilization) / rate` (seconds until 100%)
     - Shift: `prev = current`, `current = new`
   - **Unchanged:**
     - If gap >= 5 minutes → return nil (no animal displayed)
     - If gap < 5 minutes AND prev exists → keep speed from prev→current (don't update rate)
     - If gap < 5 minutes AND no prev → return nil (insufficient data)

3. **Map projectedTime to animal level:**

| Level | Animal | Projected time to 100% |
|-------|--------|------------------------|
| 1     | 🐌     | > 5 hours              |
| 2     | 🐢     | 3–5 hours              |
| 3     | 🐇     | 1.5–3 hours            |
| 4     | 🐎     | 30 min–1.5 hours       |
| 5     | 🐆     | < 30 minutes           |

### Edge Cases

- Utilization decreases (post-reset or API anomaly) → reset history for that cycle.
- Utilization = 100% → `isLimited = true` when logging, animal = 🐆.
- App freshly launched → no history → no animal until 2nd measurement.

### API

```swift
actor BurnRateTracker {
    init(logStore: UsageLogStore)

    func record(accountId: UUID, window: UsageWindow,
                utilization: Double, resetsAt: Date) -> BurnRateResult?
}

struct BurnRateResult {
    let level: Int              // 1-5
    let animal: String          // emoji
    let projectedTime: TimeInterval
}
```

---

## Section 3: View Changes — Animal on UsageBar

### UsageBar

New parameter:
```swift
var animal: String?    // nil = don't display
```

Animal emoji positioned above the progress bar at the current utilization position:
```swift
ZStack(alignment: .leading) {
    // Background bar (existing)
    // Filled bar (existing)

    if let animal {
        Text(animal)
            .offset(x: barWidth * utilization / 100 - 10, y: -18)
    }
}
```

### AccountUsageState

Extended with burn rate data:
```swift
struct AccountUsageState {
    // ... existing fields ...
    var burnRates: BurnRates?
}

struct BurnRates {
    var fiveHour: BurnRateResult?
    var sevenDay: BurnRateResult?
    var sonnet: BurnRateResult?
}
```

### AccountCard

Passes animal down to each UsageBar:
```swift
UsageBar(label: "5h", utilization: ..., animal: state.burnRates?.fiveHour?.animal)
```

Menu bar label: unchanged (too small for animals).

---

## Section 4: AccountDetailView (Drill-down)

Opens when user clicks an AccountCard.

### Layout

- **Header:** Back button, account name, plan badge
- **Segment picker:** 5h / 7d / Sonnet
- **Swift Chart (LineMark):**
  - X-axis: time, Y-axis: 0–100%
  - Limited points (100%) marked with PointMark + ⚠ annotation
  - Animal emoji displayed at data points that have burn rate
  - Hover/tap to see exact value
- **Reset cycles list:** Below chart, showing each recorded cycle with peak utilization. Click a cycle to zoom chart into that cycle.

### AccountDetailViewModel

```swift
@Observable class AccountDetailViewModel {
    let accountId: UUID
    let logStore: UsageLogStore

    var selectedWindow: UsageWindow = .fiveHour
    var logs: [UsageLogEntry] = []
    var resetCycles: [ResetCycle] = []

    func loadLogs()
    func logsForCycle(_ resetsAt: Date) -> [UsageLogEntry]
}
```

---

## Section 5: OverviewChartView (Aggregated Multi-account)

Opens from "Overview" button on DashboardWindow toolbar.

### Layout

- **Header:** Back button, "Overview" title
- **Segment picker:** 5h / 7d / Sonnet
- **Time range picker:** 24h / 3d / 7d / 30d
- **Swift Chart:**
  - **Total line (bold):** Weighted average of all selected accounts
  - **Per-account lines (thin):** Toggle on/off via legend checkboxes
  - Limited points marked with ⚠
- **Legend (below chart):** Checkbox per account + name + current animal

### Total Line Calculation

At each time point `t`:
- Get utilization of each selected account at `t` (interpolate if needed)
- `Total = weighted average` using plan weights:
  - Pro = 1, Max 5x = 5, Max 20x = 20
- Reflects total token capacity being consumed.

---

## Section 6: Integration — Data Flow

### Refresh Flow

```
DashboardViewModel.refreshAll()
  └─ for each account (parallel TaskGroup):
      1. API fetch → UsageData (existing)
      2. tracker.record(accountId, .fiveHour, utilization, resetsAt)
         ├─ Calculates burn rate → BurnRateResult?
         └─ Calls logStore.record(...) internally
      3. Repeat for .sevenDay and .sonnet
      4. Assign burnRates to AccountUsageState
  └─ Sort by burn rate (existing)
  └─ Publish → UI updates
```

### Lifecycle

```
App launch
  → UsageLogStore.init() → open/create SQLite DB, run cleanup
  → BurnRateTracker.init(logStore) → empty history
  → DashboardViewModel.init(tracker) → refreshAll()
  → Every N minutes (auto-refresh) → refreshAll() → log + calculate burn rate
```

### Estimated LOC

| File | New/Modify | LOC |
|------|-----------|-----|
| `Services/UsageLogStore.swift` | New | ~150 |
| `Services/BurnRateTracker.swift` | New | ~120 |
| `ViewModels/AccountDetailViewModel.swift` | New | ~80 |
| `Views/AccountDetailView.swift` | New | ~180 |
| `Views/OverviewChartView.swift` | New | ~200 |
| `Views/UsageBar.swift` | Modify | +15 |
| `Views/AccountCard.swift` | Modify | +5 |
| `ViewModels/DashboardViewModel.swift` | Modify | +30 |
| `Views/DashboardWindow.swift` | Modify | +10 |

### Alert

Visual only — 🐆 emoji + red color on progress bar. No macOS notifications.

### Chart Library

Swift Charts (native Apple framework, macOS 13+). No external dependencies.
