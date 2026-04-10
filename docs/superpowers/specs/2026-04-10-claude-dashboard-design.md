# Claude Dashboard — Design Spec

## Overview

Native macOS desktop app (Swift/SwiftUI) that displays Claude usage across multiple accounts on a single screen. Eliminates the need to manually check claude.ai → Settings → Usage in separate Chrome profiles.

**Target user:** Power user with 5 Claude accounts (4 Max $200/month + 1 Pro) across different Chrome profiles, using Claude Code CLI and VS Code extension.

## Requirements

### Must Have (v1)
- Display 5-hour usage % + countdown to reset for each account
- Display 7-day usage % + countdown to reset for each account
- Auto-sync authentication from Chrome profiles (no manual token input)
- Cache tokens in app's own Keychain (works without Chrome running)
- Menubar quick-glance popover with card layout
- Full window view for detailed dashboard
- Manual refresh (all accounts in parallel)
- Re-sync from Chrome when tokens expire

### Out of Scope (v1)
- Auto-refresh / polling
- Notifications when usage is high
- Usage history / trend charts
- App Store distribution
- Windows/Linux support

## Architecture

```
┌─────────────────────────────────────┐
│  Views (SwiftUI)                    │
│  ├─ MenuBarExtra (popover)          │
│  └─ NSWindow (full dashboard)       │
├─────────────────────────────────────┤
│  ViewModels                         │
│  └─ DashboardViewModel             │
├─────────────────────────────────────┤
│  Services                           │
│  ├─ ChromeCookieService             │
│  ├─ UsageAPIService                 │
│  ├─ KeychainService                 │
│  └─ AccountStore                    │
└─────────────────────────────────────┘
```

**Data flow:** ChromeCookieService → extract sessionKey + orgId → cache in Keychain → UsageAPIService calls API → ViewModel updates UI

## Chrome Cookie Sync

### How it works

1. **Scan profiles:** Read `~/Library/Application Support/Google/Chrome/Local State` (JSON) → get list of all Chrome profiles with display names
2. **Decrypt cookies:** Each profile has a `Cookies` SQLite database → query rows where `host_key` is `.claude.ai` or `claude.ai`, filtering for cookie `name` = `sessionKey` and `name` = `lastActiveOrg`
3. **Decryption steps:**
   - Get passphrase from macOS Keychain (service: `Chrome Safe Storage`)
   - Derive key: PBKDF2(passphrase, salt="saltysalt", iterations=1003, keylen=16, hash=SHA1)
   - Strip 3-byte `v10` prefix from encrypted value
   - Decrypt with AES-128-CBC, IV = 16 bytes of `0x20` (space)
   - Remove PKCS#7 padding
   - Note: Chrome DB version 24+ may prepend a 32-byte domain hash — detect and strip if present
4. **Cache:** Store session token in app's Keychain (`com.claude-dashboard.{accountId}`), orgId in UserDefaults → app runs independently of Chrome

### First-time setup flow

```
[App opens] → [Scan Chrome profiles] → [Show profiles that have claude.ai sessions]
→ [User names each account: "Max Account 1", "Pro Account", ...] → [Done]
```

### Token expiration handling

- API returns 401/403 → mark account as `.expired`
- User clicks "Re-sync from Chrome" → app re-reads cookie from mapped Chrome profile
- If Chrome session is also expired → show message asking user to login in that Chrome profile first, then re-sync

### Permissions

App runs **without App Sandbox** (required to read Chrome cookie database and Keychain). This is acceptable since the app is local-only, not distributed via App Store.

## API

### Endpoint

```
GET https://claude.ai/api/organizations/{orgId}/usage
```

### Request headers

```
accept: */*
content-type: application/json
anthropic-client-platform: web_claude_ai
Cookie: sessionKey={sessionKey}
```

### Response format

```json
{
  "five_hour": { "utilization": 42.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
  "seven_day": { "utilization": 18.0, "resets_at": "2026-04-14T16:59:59.661657+00:00" },
  "seven_day_sonnet": { "utilization": 0.0, "resets_at": null },
  "seven_day_opus": null,
  "seven_day_oauth_apps": null,
  "seven_day_cowork": null,
  "extra_usage": null
}
```

- `utilization`: Double, 0-100 (percentage)
- `resets_at`: ISO 8601 timestamp with fractional seconds (nullable)

### Session refresh

Parse `Set-Cookie` header in response for `sessionKey=...`. If found, update Keychain with new token.

### Error handling

- 401/403 → `AccountStatus.expired`
- Other HTTP errors → `AccountStatus.error` with message
- Invalid JSON → `AccountStatus.error`

## Data Models

```swift
// API response models
struct UsageLimit: Codable {
    let utilization: Double       // 0-100
    let resetsAt: Date?           // ISO 8601, JSON key: "resets_at"
}

struct UsageData: Codable {
    let fiveHour: UsageLimit      // "five_hour" — always present
    let sevenDay: UsageLimit      // "seven_day" — always present
}

// Local models
enum AccountPlan: String, Codable {
    case pro
    case max200
}

enum AccountStatus: String, Codable {
    case active
    case expired
    case error
}

struct Account: Identifiable, Codable {
    let id: UUID
    var name: String                    // User-defined label, e.g. "Max Account 1"
    var chromeProfilePath: String       // Chrome profile directory name, e.g. "Profile 1"
    var orgId: String?                  // From lastActiveOrg cookie
    var plan: AccountPlan
    var lastSynced: Date?
    var status: AccountStatus
}

// ViewModel state per account
struct AccountUsageState: Identifiable {
    let id: UUID                        // matches Account.id
    let account: Account
    var usage: UsageData?
    var isLoading: Bool
    var error: String?
}
```

## Storage

| Data | Storage | Key format |
|------|---------|------------|
| Session keys | macOS Keychain | `com.claude-dashboard.session.{accountId}` |
| Account list | UserDefaults | `claude-dashboard.accounts` (JSON array) |
| orgId per account | UserDefaults | `claude-dashboard.orgId.{accountId}` |
| Chrome profile mapping | Part of Account model | `chromeProfilePath` field |
| Window state | UserDefaults | Standard NSWindow autosave |

## UI Design

### Menubar

- Icon: small Claude-style icon
- Text: shows account with highest 5h usage: `42% · 2h15m`
- Click → popover with card layout

### Popover (MenuBarExtra, .window style)

- Header: "Claude Dashboard" + refresh button + settings button + open-window button
- Scrollable list of account cards
- Fixed width ~320pt

### Account Card

```
┌─────────────────────────────────────┐
│  Max Account 1                  🟡  │   ← name + status dot
│                                     │
│  5h  ████████░░░░  42%             │   ← progress bar + percentage
│       resets in 2h 15m              │   ← countdown
│                                     │
│  7d  ███░░░░░░░░░  18%             │
│       resets in 4d 3h               │
└─────────────────────────────────────┘
```

- Rounded rectangle, native macOS material
- Status dot + progress bar color: continuous gradient based on **5h usage only** — green (0%) transitioning smoothly through yellow/orange to red (100%). Each percentage has its own interpolated color (HSB interpolation: hue from 120° green → 0° red)
- Expired card: dimmed, shows "Session expired" + "Re-sync from Chrome" button

### Full Window (NSWindow)

- Same card layout, but can display as 2-column grid when window is wide enough
- Settings tab: manage accounts (rename, re-sync, remove), add new accounts from Chrome
- Native macOS appearance, follows system light/dark mode

### Refresh Behavior

- Manual only: user clicks 🔄 button
- Refreshes all accounts in parallel using `TaskGroup`
- Shows loading spinner on each card during refresh

## Frameworks

| Framework | Usage |
|-----------|-------|
| SwiftUI | All UI (MenuBarExtra, views, data binding) |
| Combine | @Published properties, reactive state |
| Foundation | URLSession networking, JSONDecoder, DateFormatter |
| Security | Keychain access (SecItemAdd, SecItemCopyMatching, SecItemUpdate, SecItemDelete) |
| CommonCrypto | PBKDF2 key derivation, AES-128-CBC decryption for Chrome cookies |
| SQLite3 | Reading Chrome cookie database |

Zero external dependencies. Pure Apple frameworks only.

## Project Structure

```
ClaudeDashboard/
├── ClaudeDashboard.xcodeproj/
├── ClaudeDashboard/
│   ├── ClaudeDashboardApp.swift          # @main, MenuBarExtra setup
│   ├── Info.plist                        # LSUIElement=true (no dock icon)
│   ├── Assets.xcassets/
│   ├── Models/
│   │   ├── Account.swift
│   │   ├── UsageData.swift
│   │   └── AccountUsageState.swift
│   ├── ViewModels/
│   │   └── DashboardViewModel.swift
│   ├── Services/
│   │   ├── ChromeCookieService.swift     # Chrome profile scanning + cookie decryption
│   │   ├── UsageAPIService.swift         # claude.ai API client
│   │   ├── KeychainService.swift         # macOS Keychain wrapper
│   │   └── AccountStore.swift            # Account CRUD + persistence
│   └── Views/
│       ├── MenuBarPopover.swift          # Popover content with card list
│       ├── DashboardWindow.swift         # Full window view
│       ├── AccountCard.swift             # Single account card
│       ├── UsageBar.swift                # Progress bar component
│       └── SettingsView.swift            # Account management
└── ClaudeDashboardTests/
    ├── UsageDataTests.swift              # JSON decoding tests
    ├── ChromeCookieServiceTests.swift    # Cookie decryption tests
    └── AccountStoreTests.swift           # Account persistence tests
```

## Security Considerations

- Session keys stored in macOS Keychain with `kSecAttrAccessibleAfterFirstUnlock`
- Chrome Safe Storage passphrase accessed read-only
- No data leaves the machine (all API calls are direct to claude.ai)
- No App Sandbox — required for Chrome cookie access and Keychain
- No network calls except to `claude.ai`
