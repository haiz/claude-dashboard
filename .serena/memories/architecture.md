# Architecture

## Directory Structure
```
ClaudeDashboard/
├── ClaudeDashboardApp.swift    # Entry point, MenuBarExtra + AppDelegate
├── Info.plist
├── Assets.xcassets/
├── Models/
│   ├── Account.swift           # Account, AccountPlan, AccountStatus
│   └── UsageData.swift         # UsageData, UsageLimit
├── Services/
│   ├── ChromeCookieService.swift  # Chrome cookie decryption (PBKDF2-SHA1 + AES-128-CBC)
│   ├── UsageAPIService.swift      # Claude.ai API client, plan tier detection
│   ├── KeychainService.swift      # SecItem wrapper with in-memory cache
│   └── AccountStore.swift         # CRUD over UserDefaults JSON, Combine @Published
├── ViewModels/
│   └── DashboardViewModel.swift   # @MainActor, parallel refresh, burn-rate sorting
└── Views/
    ├── MenuBarPopover.swift     # Compact menu bar dropdown
    ├── DashboardWindow.swift    # Adaptive grid of AccountCards
    ├── AccountCard.swift        # Per-account display with color progress bars
    ├── UsageBar.swift           # Color-interpolated progress bar (green→red)
    ├── SetupView.swift          # Wizard scanning Chrome profiles
    └── SettingsView.swift       # Account management (rename, delete, re-sync)
```

## Key Models
- **Account** — id, name, email, chromeProfilePath, orgId, plan (AccountPlan enum: pro/max5x/max20x/max200), status (AccountStatus)
- **UsageData** — fiveHour, sevenDay, sevenDaySonnet (UsageLimit entries)
- **UsageLimit** — utilization (Double), resetsAt (Date)

## Key Services
- **ChromeCookieService** (enum) — Static methods. Scans Chrome profiles, extracts/decrypts cookies using Chrome Safe Storage password from Keychain
- **UsageAPIService** (class) — Fetches `/api/organizations/{orgId}/usage`, detects plan tier from `extra_usage`, handles session key refresh via Set-Cookie
- **KeychainService** (enum) — Static methods with `servicePrefix` and `cache`. save/load/delete/sessionKey
- **AccountStore** (class, ObservableObject) — @Published accounts, CRUD, UserDefaults persistence

## ViewModel
- **DashboardViewModel** — @MainActor, parallel refresh via TaskGroup, sorts by burn rate (utilization / time-remaining), computes menuBarLabel
- **AccountUsageState** — Combines account, usage, loading, and error state

## Entry Point
- **ClaudeDashboardApp** — SwiftUI App with MenuBarExtra, uses AppDelegate for window lifecycle
- **AppDelegate** — Manages dashboardWindow (NSWindow)
- **DashboardWindowWrapper** — Bridge between MenuBarExtra and main window
