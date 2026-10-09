# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

macOS menu bar app (SwiftUI) that monitors Claude.ai token usage across multiple accounts. Extracts session keys from the encrypted cookie database of a supported Chromium browser (Chrome, Arc, Brave, or Edge), fetches usage data from Claude.ai API, and displays real-time utilization with burn-rate-based sorting.

## Build & Test Commands

```bash
# Generate Xcode project from project.yml (required after adding files/targets)
cd apps/macos && xcodegen generate

# Build
xcodebuild -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build

# Run all tests (app + Shared)
xcodebuild -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test

# Run the helper CLI tests — a separate bundle; the scheme above never compiles Helper/
xcodebuild -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardHelperTests test

# Run a single test class
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageDataTests

# Run a single test method
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageDataTests/testDecodeUsageData

# Run the GNOME Shell extension's test suite (apps/linux/)
cd apps/linux && CLAUDE_DASHBOARD_REPO="$(git rev-parse --show-toplevel)" gjs -m tests/run.js

# Windows: core on Windows (paths, DPAPI, v10 cookies)
cd apps/linux && cargo test -p claude-dashboard-core
# Windows: the bridge workspace (framing, sources, handler, roundtrip)
cd apps/windows && cargo test --workspace
# Windows: the browser extension
cd apps/windows/extension && node --test
# Windows: run the Slint app (CLAUDE_DASHBOARD_FAKE_ROWS=1 seeds sample cards, no network)
cd apps/windows && cargo run -p claude-dashboard
# Windows: build the per-user MSI (needs WiX 3.14 + cargo-wix 0.3.9; output apps/windows/target/wix/)
powershell -File apps/windows/scripts/build-msi.ps1
# Windows: check the extension zip packer
powershell -File apps/windows/scripts/test-pack-extension.ps1
# Windows: check the one-liner installer (install.ps1), offline
powershell -File apps/windows/scripts/test-install-ps1.ps1
```

No external dependencies — pure native Swift (SwiftUI, AppKit, Combine, Security, CommonCrypto, SQLite3).

## Architecture

**Data flow:** Browser cookies — Chrome/Arc/Brave/Edge (SQLite + AES decryption) → Session keys (AES-GCM encrypted in UserDefaults) → Claude.ai API → Usage data → ViewModel → SwiftUI views

### Services Layer
- **BrowserCookieService** — Decrypts the SQLite cookie DB of Chrome, Arc, Brave, or Edge using PBKDF2-SHA1 + AES-128-CBC. The Safe Storage password is read from the Keychain per-browser, under that browser's own service name (e.g. "Chrome Safe Storage", "Brave Safe Storage" — see `Browser.swift`). Copies DB to avoid WAL locks.
- **UsageAPIService** — Fetches `/api/organizations/{orgId}/usage` from claude.ai. Plan tier
  comes from the **organizations** endpoint's `capabilities` (`claude_pro` / `claude_max`),
  never from the usage response — `extra_usage` is a pay-as-you-go overage toggle with no
  tier field. The API exposes no Max 5x vs 20x signal, so consumer Max accounts resolve to
  the generic `Max`. Handles session key refresh via Set-Cookie headers.
  Account identity comes from `/api/account` (`fetchAccount`), which returns the
  account's own `uuid` and `email_address` — `orgId` identifies an organisation,
  not an account, and is never used for dedupe. See `contract/README.md`'s
  "Account identity" and "Org selection" sections.
- **CryptoService** — AES-GCM encryption of session keys at rest, with the key derived via HKDF from the machine's `IOPlatformUUID`. Session keys live inside the `Account` JSON in UserDefaults, not in the Keychain.
- **AccountStore** — CRUD over UserDefaults JSON persistence. Publishes changes via Combine `@Published`.
- **ClaudeCodeSwitcher** (`Services/ClaudeCodeSwitch/`) — one-click switch of the account Claude
  Code uses in `~/.claude`. Each refresh copies the active `claudeAiOauth` from the
  `Claude Code-credentials` Keychain entry into a per-account vault (`ClaudeDashboard.cc-vault`),
  since Claude Code rotates refresh tokens; Switch saves the active account, writes the target's
  credential (keeping `mcpOAuth`) and its `oauthAccount` in `~/.claude.json`. Capture and switch
  are serialized by a lock. Switch refuses when Claude Code's active login is not a dashboard
  account or `~/.claude.json` names no account, while it holds a live login. All Keychain access
  goes through `/usr/bin/security` (no access prompt); writes send hex on stdin of `security -i` when the
  command line is at most 4032 characters (it truncates longer lines), otherwise hex in argv,
  matching Claude Code's own rule. Disabled under XCTest (`ClaudeCodeSwitcher.live(isRunningTests:)`
  returns nil). Do not also use the same account through another `CLAUDE_CONFIG_DIR`: two copies
  of one refresh-token chain kill each other.
  Spec: `docs/superpowers/specs/2026-10-08-claude-code-account-switch-design.md`.
- **ClaudeCodeLoginProvisioner** (`Services/ClaudeCodeSwitch/`) — mints a Claude Code login so
  Switch needs no `/login`, **silently only**. `ClaudeAIGrantClient` asks claude.ai
  (`/v1/oauth/{org}/authorize`, undocumented) for a code with the account's `sessionKey`;
  `ClaudeCodeOAuthClient` exchanges it at Claude Code's own token endpoint and builds the
  `claudeAiOauth`+`oauthAccount` pair (OAuth constants read from the Claude Code binary).
  claude.ai grants only when the browser signed in recently (`session_stale_for_elevated_grant`
  / 403 `session_stale_relogin`); on that gate `provisionSilently` returns nil and the dashboard
  does NOT open a browser (the re-login for these group-email accounts is an email magic link —
  a bad mid-switch interruption). A Switch tap on an uncaptured/expired account provisions then
  switches, or shows "sign in to claude.ai … then Switch again" when stale; refresh provisions
  silently ≤1/hour/account, so an account the user just signed into lights up on its own. The
  minted email must match the account or the entry is discarded. Nil under XCTest, like the
  switcher. (Git history has an earlier browser-popup approach with `OAuthCallbackListener` /
  `BrowserProfileOpener`, removed because the consent page just bounced to the magic-link login.)

### ViewModel
- **DashboardViewModel** — `@MainActor` observable. Parallel refresh via `TaskGroup`. Sorts accounts by burn rate (utilization / time-remaining). Computes menu bar label from highest utilization.

### Views
- **ClaudeDashboardApp** — Entry point. `MenuBarExtra` with `AppDelegate` managing window lifecycle.
- **MenuBarPopover** — Compact menu bar dropdown. Expand button opens `MainWindow`.
- **MainWindow** — System Settings-style `NavigationSplitView`. `SidebarView` lists Usage (Dashboard, Overview), Accounts (one row per account), Tools (Command Log, Help) and Settings (Accounts, General); `DashboardViewModel.selection: SidebarItem` picks the pane. Each pane starts with a `PaneHeader` (title + its buttons; the AppKit window has no native toolbar).
- **DashboardPane / AccountPane** — the `AccountCard` grid, and the per-account page (header, `UsageGaugeRow`, the `AccountDetailView` chart, actions).
- **AccountCard / UsageBar** — Per-account display with color-interpolated progress bars (green→red).
- **SetupView** — Wizard scanning browser profiles for active Claude sessions. Offers the installed browsers (Chrome, Arc, Brave, Edge) and scans the one the user picks, remembering the choice in `preferredScanBrowser`.
- **Settings panes** (`Views/Settings/`) — `AccountsSettingsPane` (add via the `SetupView` sheet, delete, Re-sync All) and `GeneralSettingsPane` (About header, Auto Refresh, Updates). There is no rename UI; an account's name is derived at sync time (email when available, else the browser profile name).

### Tests
- **ClaudeDashboardTests** — the app bundle, hosted by `ClaudeDashboard.app`.
- **ClaudeDashboardHelperTests** — a second bundle with no test host, compiling
  `Helper/` (minus `main.swift`) and `Shared/` directly; this is the only way
  anything under `Helper/` is reachable from `xcodebuild test`. `SyncCommand` is
  driven through `runAsync(env:)` with an injected `Environment`, so tests never
  touch a browser, the Keychain, UserDefaults or the network. Code shared by both
  bundles lives in `ClaudeDashboardTests/TestSupport/` and nowhere else.

### Models
- **Account** — Core model with `AccountPlan` enum (pro/max5x/max20x/max200) and `AccountStatus` (active/expired/error).
- **UsageData** — Decoded API response with `UsageLimit` entries for the 5-hour and 7-day
  windows, plus an optional Fable window. Fable has no top-level field: it is derived from
  the `limits` array entry whose `scope.model.display_name` is `"Fable"`, reading `percent`
  rather than `utilization`. `seven_day_sonnet` is a removed field the decoder ignores.

### Linux
- **apps/linux/lib/** — canonical pure modules (no `gi://` or `resource:///org/gnome/shell` imports)
  ported from macOS: ring-gauge geometry, colors, and burn-rate math. Consumed by both the
  GNOME extension and the GTK app. The extension reaches `lib/` via symlink at
  `apps/linux/gnome-extension/lib` → `../lib`.
- **apps/linux/tests/** — test suite (ESM, run under `gjs`) driving the pure modules. Run with
  `cd apps/linux && CLAUDE_DASHBOARD_REPO="$(git rev-parse --show-toplevel)" gjs -m tests/run.js`.
- **apps/linux/gnome-extension/** — a GNOME Shell panel indicator driving the same
  `claude-dashboard-helper` binary the CLI uses.

### Windows
- **apps/windows/** — a separate Cargo workspace (Rust) for the Windows port. It reuses
  `apps/linux/core` by path dependency (no crate move), so the Linux CI and release flow are
  untouched. Sub-project 1 delivers the bridge and extension; sub-project 2 the Slint app:
  - **apps/windows/app/** — `claude-dashboard`, the Slint UI binary: main window with Mica and
    system light/dark theme, sidebar, Dashboard card grid with core-driven ring gauges,
    per-account page, tray ring icon (peak usage) with a Mica flyout popover, a background
    refresh loop, a single-instance mutex, and the consumer of the `\\.\pipe\claude-dashboard`
    reload pipe. Closing the window hides it; the app lives in the tray. GUI rendering is
    verified by run-checks, not CI. Sub-project 3 adds the Add Account wizard (extension / scan /
    paste tabs) and the Settings Accounts (delete + mute, re-sync) and General (auto refresh,
    launch at startup) panes. Sub-project 4 adds charts: the per-account interactive usage chart
    (`ui/chart.slint`, `src/chart.rs`, pure helpers in `chart_model.rs`), opened by the "View
    chart" button, and the Overview multi-account chart opened from the sidebar "Overview" item. Sub-project 5 adds
    the Command Log pane (Tools), the per-account Run Command panel (saved command, Open in
    Terminal toggle, classifier-driven default), auto-run on reset (hidden, once per episode), the
    shell picker (Settings > General > Commands), the green Claude Code badge, and the Help pane
    (`log_view.rs`, `run_command.rs`); the sidebar "Coming soon" placeholder is gone. See
    `contract/windows.md`, "Command Log". Sub-project 6 adds release: a per-user MSI
    (`app/wix/main.wxs`, built by `scripts/build-msi.ps1`; relative-path host manifest, fixed
    UpgradeCode and component GUIDs, uninstall keeps user data), the extension zip
    (`scripts/pack-extension.ps1`), auto-update (`app/src/updater.rs` for guards, MSI download
    checks and the detached relauncher, over `core::update` and the `autoUpdate` /
    `lastAutoUpdateCheck` settings; installed copy only, `DISABLE_AUTOUPDATER=1` disables) and
    the `release-windows.yml` workflow. See `contract/windows.md`, "Installer", "Updates", "Release".
  - **apps/windows/bridge/** — `claude-dashboard-bridge.exe`, a native-messaging host for the
    browser extension. `handler.rs` runs the key intake over an injected `Environment` trait
    (so tests touch no network/store/pipe); `real_env.rs` is the production wiring; `framing.rs`
    speaks the Chrome native-messaging length-prefix framing; `sources.rs` owns
    `extension-sources.json` (install→account bindings and muted installs); `notify.rs` pokes
    the app's named pipe. See `contract/windows.md`.
  - **apps/windows/extension/** — an MV3 browser extension (ES modules) that reads the claude.ai
    `sessionKey` cookie via `chrome.cookies` and forwards it to the bridge. Logic lives in
    `lib/` and is tested with `node --test`; the extension id is fixed by the manifest `key`.
- **Windows-specific `core` modules:** `store.rs` resolves `%APPDATA%`/`%LOCALAPPDATA%` and seals
  session keys with DPAPI (`cfg(windows)`); `userprotect.rs` wraps `CryptProtectData`;
  `cookie/win.rs` decodes `v10` cookies and refuses `v20` app-bound ones; `browser.rs` gains
  `discover_windows_profiles_under`. The add/repair logic shared by the helper's `add-key` and
  the bridge lives in `core::key_intake`. The cross-process store lock is `store::lock_store`.
- **Presentation `core` modules** (ported from `apps/linux/lib/`, same values, tested): `colors`
  (usage color interpolation), `geometry` (ring-gauge geometry), `format` (percent/reset text),
  `rows` (`DisplayRow` view models and burn-rate ordering).
- **Setup & Settings `core` modules:** `settings.rs` (`settings.json`: auto refresh, preferred scan
  browser, launch at startup; non-critical, missing/corrupt -> defaults); `scan.rs` (scans Windows
  profiles and classifies each as `ScannedSession` / `AppBound` / `NoSession`); `startup.rs`
  (launch-at-startup via the HKCU `Run` key).
- **Chart `core` modules:** `chart` (chart math ported from `apps/linux/lib/chart.js`: scale, ticks,
  zoom, reset-split segments) and the `UsageLogStore::series`/`series_all` read of the usage log
  that feeds it. See `contract/usage-log.md`.
- **Command `core` modules:** `command_log` (SQLite `command_logs.db`: trigger/status vocabulary
  with fixed raw values, newest 500 rows by id, 4096-byte output tail), `command_classifier`
  (Open-in-Terminal default from the leading token, claude print mode, TUI list, ssh walk),
  `auto_run` (`should_run_saved_command` plus the once-per-episode latch, armed only for accounts
  with a saved command), `claude_code` (active email from `%USERPROFILE%\.claude.json`) and
  `run_commands` (`run-commands.json`, keyed by account id, dropped on account delete).
- **App process control:** `shell.rs` (shell discovery, the `settings.json` `shell` choice, hidden-run
  and resolver invocations per shell), `terminal.rs` (interactive launch via `wt.exe new-tab`, else
  `conhost.exe`; cmd's `/k` tail passed raw, `;` escaped under wt), `runner.rs` (Job Object runner:
  spawn suspended, assign to a `KILL_ON_JOB_CLOSE` job, resume; 60 s timeout, cancel, background
  leftovers ended when the shell exits) and `commands.rs` (glues resolve, classify, run and log).

## Key Technical Details

- **LSUIElement: true** in Info.plist — app runs as menu bar only (no Dock icon)
- **App Sandbox disabled** — required for browser cookie DB access and Keychain reads (each browser's Safe Storage password)
- **Deployment target:** macOS 13.0, Swift 5.0
- **XcodeGen** manages the `.xcodeproj` from `project.yml` — edit `project.yml` for target/build setting changes, then run `xcodegen generate` from `apps/macos/`
- Tests use `MockURLProtocol` for network mocking and isolated `UserDefaults` suites
- **App code reads `AppDefaults.shared`, never `UserDefaults.standard`** — the
  `ClaudeDashboardTests` scheme is hosted by `ClaudeDashboard.app`, so every
  `xcodebuild test` launches the real app and runs its startup writes.
  `AppDefaults` diverts those to a suite of its own under XCTest (or to
  `CLAUDE_DASHBOARD_DEFAULTS_SUITE`, the variable the helper CLI reads). A new
  `UserDefaults.standard` in the app target escapes that guard silently
- **ISO8601 date parsing** — Custom decoder handles both with and without fractional seconds (`.SSS`); this is a known API inconsistency
- **`contract/` is the source of truth for cross-platform behaviour** — plan detection, the
  Fable window, burn-rate thresholds, the helper CLI surface, and the account schema. The
  Swift tests under `apps/macos/ClaudeDashboardTests/*ContractTests.swift` read
  `contract/cases/*.json` directly from the working tree; the Linux implementation reads the
  same files. Change a rule there first, then both implementations.

## Releasing

When asked to create a release:

1. Determine the new semver (ask if unclear).
2. Commit any uncommitted changes first.
3. Generate release notes from the git log since the last tag:
   ```bash
   git log "v$(cat VERSION)..HEAD" --pretty=format:"%s" --no-merges | grep -v "chore: release"
   ```
   Write 2-5 plain-English bullet points from a user perspective (what users notice/benefit from). Use `-` bullets, no header.
4. Pass the notes directly to the script:
   ```bash
   ./scripts/release.sh <new-version> --notes "- Fixed X\n- Added Y"
   ```

The script handles everything else: version bump, sync, xcodegen, build (app + helper), tests, artifact creation, Homebrew sha256 update, commit, tag, push, and `gh release create`.

If no `--notes` flag is passed, the script falls back to GitHub's auto-generated notes.

**Manual steps (only if the script fails):**

1. Edit `VERSION` to the new semver, then run `./scripts/sync-version.sh`.
2. Build `ClaudeDashboard` and `ClaudeDashboardHelper` schemes (Release config).
3. Create artifacts — names matter:
   - `ClaudeDashboard.app.zip` (Cask URL expects this exact name)
   - `claude-dashboard-cli.tar.gz` (must contain both `claude-dashboard-cli` AND `claude-dashboard-helper`)
4. Update `sha256` in `Formula/claude-dashboard-cli.rb` and `Casks/claude-dashboard.rb`.
5. Commit, tag, push, `gh release create` with both artifacts.

After `release.sh` publishes, `.github/workflows/release-windows.yml` builds and attaches `ClaudeDashboard-x64.msi` and `claude-dashboard-extension.zip`; if it fails, re-run it by hand with `workflow_dispatch` and the tag. `scripts/test-release-workflow.sh` enforces its upload-only guardrails.

Validate version sync at any time with `./scripts/test-sync-version.sh`.
