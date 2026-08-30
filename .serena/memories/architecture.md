# Architecture

## Directory Structure
```
apps/macos/
├── ClaudeDashboard/
│   ├── ClaudeDashboardApp.swift    # Entry point, MenuBarExtra + AppDelegate
│   ├── Info.plist
│   ├── Assets.xcassets/
│   ├── Models/
│   │   ├── AccountBadgeColor.swift
│   │   ├── CommandLogModels.swift
│   │   └── UsageLogModels.swift       # BurnRateResult, burn-rate levels/animals
│   ├── Services/
│   │   ├── AccountStore.swift         # CRUD over UserDefaults JSON, Combine @Published
│   │   ├── BurnRateTracker.swift
│   │   ├── ClaudeCodeAccountDetector.swift
│   │   ├── CommandClassifier.swift
│   │   ├── CommandLogStore.swift
│   │   ├── CommandRunner.swift
│   │   ├── KeychainService.swift      # actor; unreferenced by production code (dead code)
│   │   ├── ProcessTree.swift
│   │   ├── RunningProcessRegistry.swift
│   │   ├── TerminalLauncher.swift
│   │   ├── UpdateService.swift
│   │   └── UsageLogStore.swift
│   ├── ViewModels/
│   │   ├── AccountDetailViewModel.swift
│   │   ├── CommandLogViewModel.swift
│   │   ├── DashboardViewModel.swift   # @MainActor, parallel refresh, burn-rate sorting
│   │   └── UpdateViewModel.swift
│   └── Views/
│       ├── AccountCard.swift          # Per-account display with color progress bars
│       ├── AccountDetailView.swift
│       ├── CommandLogView.swift
│       ├── Components/HoverableButtonStyle.swift
│       ├── DashboardWindow.swift      # Adaptive grid of AccountCards
│       ├── HelpView.swift
│       ├── InteractiveChartContainer.swift
│       ├── MenuBarPopover.swift       # Compact menu bar dropdown
│       ├── OverviewChartView.swift
│       ├── RunCommandSheet.swift
│       ├── SettingsView.swift         # Account management (rename, delete, re-sync)
│       ├── SetupView.swift            # Wizard scanning browser profiles
│       └── UsageBar.swift             # Color-interpolated progress bar (green→red)
├── Shared/                            # Used by both the app target and the Helper CLI binary
│   ├── Account.swift                  # Account, AccountPlan, AccountStatus
│   ├── AppVersion.swift
│   ├── Browser.swift                  # Browser enum: chrome/arc/brave/edge, base path + Keychain service name per case
│   ├── ChromeCookieService.swift      # declares `enum BrowserCookieService` — multi-browser cookie decryption (PBKDF2-SHA1 + AES-128-CBC)
│   ├── CryptoService.swift            # AES-GCM session-key encryption, key via HKDF from IOPlatformUUID
│   ├── UsageAPIService.swift          # Claude.ai API client, plan tier detection
│   └── UsageData.swift                # UsageData, UsageLimit
└── Helper/                            # claude-dashboard-helper CLI binary
    ├── main.swift
    ├── DecryptCommand.swift
    ├── HelperAccountStore.swift
    ├── SyncCommand.swift
    └── UsageCommand.swift
```

## Key Models
- **Account** — id, name, email, chromeProfilePath, orgId, plan (AccountPlan enum: pro/max5x/max20x/max200), status (AccountStatus)
- **UsageData** — fiveHour, sevenDay, fable (optional; UsageLimit entries)
- **UsageLimit** — utilization (Double), resetsAt (Date)

## Key Services
- **BrowserCookieService** (enum, in `ChromeCookieService.swift`) — Static methods. Scans profiles and extracts/decrypts cookies for Chrome, Arc, Brave, or Edge, reading each browser's Safe Storage password from the Keychain under that browser's own service name
- **UsageAPIService** (class) — Fetches `/api/organizations/{orgId}/usage`; plan tier comes from the `/api/organizations` endpoint's `capabilities` (`claude_pro`/`claude_max`), never from `extra_usage` (a pay-as-you-go overage toggle with no tier signal); handles session key refresh via Set-Cookie
- **KeychainService** (actor) — SecItem wrapper with `servicePrefix` and an in-memory `cache`; `shared` singleton. save/load/delete/sessionKey. Currently unreferenced by production code — see `contract/account-schema.md`
- **AccountStore** (class, ObservableObject) — @Published accounts, CRUD, UserDefaults persistence

## ViewModel
- **DashboardViewModel** — @MainActor, parallel refresh via TaskGroup, sorts by burn rate (utilization / time-remaining), computes menuBarLabel
- **AccountUsageState** — Combines account, usage, loading, and error state

## Entry Point
- **ClaudeDashboardApp** — SwiftUI App with MenuBarExtra, uses AppDelegate for window lifecycle
- **AppDelegate** — Manages dashboardWindow (NSWindow)
- **DashboardWindowWrapper** — Bridge between MenuBarExtra and main window
