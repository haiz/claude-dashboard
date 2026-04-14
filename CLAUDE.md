# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

macOS menu bar app (SwiftUI) that monitors Claude.ai token usage across multiple accounts. Extracts session keys from Chrome's encrypted cookie database, fetches usage data from Claude.ai API, and displays real-time utilization with burn-rate-based sorting.

## Build & Test Commands

```bash
# Generate Xcode project from project.yml (required after adding files/targets)
xcodegen generate

# Build
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build

# Run all tests
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test

# Run a single test class
xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageDataTests

# Run a single test method
xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageDataTests/testDecodeUsageData
```

No external dependencies — pure native Swift (SwiftUI, AppKit, Combine, Security, CommonCrypto, SQLite3).

## Architecture

**Data flow:** Chrome cookies (SQLite + AES decryption) → Session keys (Keychain) → Claude.ai API → Usage data → ViewModel → SwiftUI views

### Services Layer
- **ChromeCookieService** — Decrypts Chrome's SQLite cookie DB using PBKDF2-SHA1 + AES-128-CBC with Chrome Safe Storage password from Keychain. Copies DB to avoid WAL locks.
- **UsageAPIService** — Fetches `/api/organizations/{orgId}/usage` from claude.ai. Detects plan tier (Pro/Max 5x/Max 20x) from `extra_usage` response field. Handles session key refresh via Set-Cookie headers.
- **KeychainService** — Wraps SecItem APIs with in-memory cache for session keys.
- **AccountStore** — CRUD over UserDefaults JSON persistence. Publishes changes via Combine `@Published`.

### ViewModel
- **DashboardViewModel** — `@MainActor` observable. Parallel refresh via `TaskGroup`. Sorts accounts by burn rate (utilization / time-remaining). Computes menu bar label from highest utilization.

### Views
- **ClaudeDashboardApp** — Entry point. `MenuBarExtra` with `AppDelegate` managing window lifecycle.
- **MenuBarPopover** — Compact menu bar dropdown. Expand button opens `DashboardWindow`.
- **DashboardWindow** — Adaptive grid of `AccountCard` views with toolbar.
- **AccountCard / UsageBar** — Per-account display with color-interpolated progress bars (green→red).
- **SetupView** — Wizard scanning Chrome profiles for active Claude sessions.
- **SettingsView** — Account management (rename, delete, re-sync).

### Models
- **Account** — Core model with `AccountPlan` enum (pro/max5x/max20x/max200) and `AccountStatus` (active/expired/error).
- **UsageData** — Decoded API response with `UsageLimit` entries for 5-hour, 7-day, and Sonnet windows.

## Key Technical Details

- **LSUIElement: true** in Info.plist — app runs as menu bar only (no Dock icon)
- **App Sandbox disabled** — required for Chrome cookie DB access and Keychain reads
- **Deployment target:** macOS 13.0, Swift 5.0
- **XcodeGen** manages the `.xcodeproj` from `project.yml` — edit `project.yml` for target/build setting changes, then run `xcodegen generate`
- Tests use `MockURLProtocol` for network mocking and isolated `UserDefaults` suites
- **ISO8601 date parsing** — Custom decoder handles both with and without fractional seconds (`.SSS`); this is a known API inconsistency

## Releasing

Run the automated release script:

```bash
./scripts/release.sh <new-version>   # e.g. ./scripts/release.sh 1.3.0
```

The script handles everything: version bump, sync, xcodegen, build (app + helper), tests, artifact creation with correct names/contents, Homebrew sha256 update, commit, tag, push, and `gh release create`.

**Manual steps (only if the script fails):**

1. Edit `VERSION` to the new semver, then run `./scripts/sync-version.sh`.
2. Build `ClaudeDashboard` and `ClaudeDashboardHelper` schemes (Release config).
3. Create artifacts — names matter:
   - `ClaudeDashboard.app.zip` (Cask URL expects this exact name)
   - `claude-dashboard-cli.tar.gz` (must contain both `claude-dashboard-cli` AND `claude-dashboard-helper`)
4. Update `sha256` in `Formula/claude-dashboard-cli.rb` and `Casks/claude-dashboard.rb`.
5. Commit, tag, push, `gh release create` with both artifacts.

Validate version sync at any time with `./scripts/test-sync-version.sh`.
