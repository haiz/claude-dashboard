# Windows App: Native Windows 11 Port at Feature Parity with macOS

## Summary

A Windows build of Claude Dashboard with the same feature set as the macOS app:
tray quick view, main window (Dashboard, Overview, per-account pages), charts,
Command Log, Help, Setup and Settings, auto-update.

- **Look:** the macOS layout and components (sidebar with colored tiles,
  account cards, ring gauges, green→red interpolation, avatars, burn-rate
  animals), rendered with Windows 11 materials — Mica, Fluent controls,
  Segoe UI Variable, Segoe Fluent Icons.
- **Stack:** pure Rust. UI in Slint with its `fluent` style; logic from the
  existing `apps/linux/core` crate, called in-process.
- **Session keys:** a companion browser extension reads the claude.ai
  `sessionKey` cookie through `chrome.cookies` and hands it to the app over
  Native Messaging. Pasting a key by hand stays as a fallback; scanning the
  cookie DB works only for `v10` (DPAPI) cookies.

### Decisions and their reasons

| Decision | Reason |
|---|---|
| Feature parity, not an MVP | Requested scope. Delivered as ordered sub-projects (below), each shippable. |
| Native Win11 materials, macOS layout | Recognisably the same app, without looking foreign on Windows. |
| Browser extension for keys | Chrome 127+ on Windows uses app-bound encryption (`v20` cookies); the key is bound to Chrome via a SYSTEM elevation service. Getting around it means impersonating or injecting into the browser — infostealer technique, flagged by AV, and Google keeps tightening it. `chrome.cookies` is the supported API. |
| Slint (fluent style) over WinUI 3 / Tauri / egui | One language with the existing Rust core; Slint ships a Fluent style; declarative `.slint` is closest to SwiftUI. egui cannot reach the Fluent look. |
| Reuse `apps/linux/core` by path dependency | No crate move, so the Linux CI and release workflow are untouched. |

### Assumptions

- Windows 11 only. On Windows 10 the window falls back to a solid background
  (no Mica); no other Win10 work is planned.
- Supported browsers: Chrome, Edge, Brave, Arc (all Chromium; the extension
  runs in each).
- Public release alongside the macOS and Linux builds.

## Sub-projects

Each gets its own implementation plan. Order is dependency order.

1. **Core on Windows + bridge** — Windows paths and DPAPI in `core`, `v10`
   cookie decryption, the native-messaging bridge, the extension, `core` tests
   green on Windows.
2. **App shell** — tray icon, popover, main window with sidebar, Dashboard
   grid, per-account page, refresh loop.
3. **Setup and Settings** — Add Account (extension / scan / paste), Accounts
   and General panes, launch at startup, muted sources.
4. **Charts** — Overview and per-account charts on the usage log.
5. **Command Log** — run commands per account, auto triggers, Job Object
   process control, Windows Terminal launch, log view; Help.
6. **Release** — MSI, auto-update, `release-windows.yml`, version sync.

## Architecture

```
apps/windows/                     new Cargo workspace
  app/        claude-dashboard.exe        Slint UI, tray, refresh loop
    ui/*.slint                            sidebar, card, gauge, popover, chart, settings
    src/                                  view-model, tray, popover placement, Mica
  bridge/     claude-dashboard-bridge.exe native-messaging host
  extension/                              MV3 browser extension
apps/linux/core/                  shared; gains cfg(windows) modules
```

### `core` changes (`apps/linux/core`)

- **`store.rs`:** on Windows, `accounts.json` lives in
  `%APPDATA%\claude-dashboard\` and `usage_logs.db` in
  `%LOCALAPPDATA%\claude-dashboard\`. Session keys at rest are protected with
  DPAPI (`CryptProtectData`, current-user scope) instead of the machine-id
  derived key. The Unix paths and tests stay as they are, gated `cfg(unix)`.
- **`cookie/`:** a Windows branch reads `os_crypt.encrypted_key` from the
  profile's `Local State`, unwraps it with DPAPI and decrypts `v10` values
  with AES-256-GCM. A `v20` value returns a distinct error,
  `AppBoundEncrypted`, so the UI can point the user to the extension.
- **`browser.rs`:** Windows profile roots under `%LOCALAPPDATA%` for Chrome,
  Edge, Brave and Arc.

### In-process, not via the helper CLI

The app links `core` directly; it does not shell out to
`claude-dashboard-helper` the way the GNOME extension does. The refresh loop
runs on a background thread and hands results to Slint with
`slint::invoke_from_event_loop`.

### Contract

Add a `contract/windows.md`: data paths, the native-messaging message shapes
(below), the bridge's add/update rules, and the muted-source rule. Business
rules (plan detection, dedupe, org selection, burn rate, Fable window) are
unchanged and stay covered by the existing contract cases, which now also run
on Windows.

## UI

| macOS | Windows |
|---|---|
| Menu bar % label | **Tray icon drawn at runtime** (`tiny-skia`): a small ring filled to the highest utilisation, colored by the shared green→red interpolation. Tooltip: `Claude: 82% · resets in 1h12m`. |
| `MenuBarPopover` | **Left click:** borderless, rounded, Mica flyout anchored to the tray corner of the work area (like the Win11 volume/network flyouts); closes on focus loss. Same content: header, account rows with gauges, expand button. **Right click:** menu with Open Dashboard / Refresh / Quit. |
| `MainWindow` | Mica window with a custom title bar. Sidebar groups Usage (Dashboard, Overview), Accounts (one row per account), Tools (Command Log, Help), Settings (Accounts, General), with the same tile colors (blue, orange, indigo, green, teal, gray). |
| `PaneHeader` | Pane title and its buttons, placed in the title-bar area. |
| `AccountCard`, `UsageGaugeRow`, `UsageBar`, `AccountAvatar` | Ported 1:1 as `.slint` components. |
| SF Symbols | Segoe Fluent Icons (ships with Windows 11). |
| Light/dark | Follows the system setting and switches live. |

- **One source for colors and geometry:** color interpolation, ring-gauge
  geometry and avatar colors are ported from `apps/linux/lib/` into a Rust
  module, with tests asserting the same values as the macOS implementation.
- **Motion:** gauges animate value changes (Slint `animate`); cards highlight
  on hover.
- **Type:** Segoe UI Variable; tabular figures for countdowns.

## Data flow

```
timer (Auto Refresh) → background thread: core::api per account, in parallel
  → usage log (SQLite) → burn rate + sort (core) → invoke_from_event_loop → Slint models
```

- A key rotated through `Set-Cookie` is persisted by `core`, as on macOS.
- 401/403 marks the account `expired`. The card's hint depends on its source:
  extension → "open claude.ai in <browser>", pasted → "paste the key again".
- On a network or API error the last data stays visible, the card shows
  "updated 10:42" and a short error.

## Session keys: extension → bridge → app

### Extension (MV3)

- Permissions: `cookies`, `nativeMessaging`, `alarms`, `storage`; host
  permission `https://claude.ai/*`.
- Sends the key on install, on `chrome.cookies.onChanged` for the claude.ai
  `sessionKey` cookie, and every 30 minutes through `alarms`.
- Each install keeps a random `installId` in `storage.local`, so every browser
  profile is a distinct source.
- Message: `{ "type": "sessionKey", "installId", "browser", "sessionKey" }`.
  The reply is `{ "ok": true, "email" }` or
  `{ "ok": false, "error": "<code>", "message" }`.
- Badge `!` and a popup explanation when the host is missing or replies with
  an error; otherwise the popup shows "Synced user@x.com at <time>".
- Fixed extension ID via the manifest `key` field, so the unpacked build has a
  stable ID.

### Bridge (`claude-dashboard-bridge.exe`)

- Speaks the Native Messaging framing (4-byte little-endian length + JSON) on
  stdin/stdout.
- Runs the same logic as `add-key`: `/api/account` for uuid and email, dedupe
  per contract, org selection, plan detection. Then adds or updates the
  account with `source: "extension"`, recording the `installId` and browser.
- An `installId` on the muted list is acknowledged and ignored.
- Notifies a running app through the named pipe `\\.\pipe\claude-dashboard`
  (message: `reload`). A missing pipe is not an error; the app reads the store
  on its next start.
- All side effects go through an injected `Environment` (store, API, pipe), as
  `SyncCommand` does on macOS, so tests touch none of them.

### Registration

The installer writes the host manifest next to the bridge and registers it
under `HKCU\Software\Google\Chrome\NativeMessagingHosts\<name>` (Chrome,
Brave, Arc) and `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\<name>`.
`allowed_origins` lists the unpacked ID and reserves the Chrome Web Store and
Edge Add-ons IDs.

### Concurrent writes

Bridge and app both write `accounts.json`. Writers take an exclusive lock on a
sibling lock file, then write a temp file and rename it, as `save_accounts`
does today.

## Setup and Settings

**Add Account** (macOS `SetupView`), three tabs:

1. **Browser extension** (default): detects installed browsers, links to the
   store page or unpacked-install steps, shows live status ("Waiting for
   key…" → "Added user@x.com").
2. **Scan browser:** succeeds for `v10` cookies only; on `AppBoundEncrypted`
   it says so and points to tab 1.
3. **Paste session key:** as `PasteKeyView`.

**Settings › Accounts:** list, delete, Re-sync All, muted sources (unmute).
Deleting an extension-sourced account mutes its `installId`.

**Settings › General:** About, Auto Refresh, Launch at startup
(`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`), Updates.

## Charts

- Data from `usage_logs.db` via `core`'s usage log, schema per
  `contract/usage-log.md`.
- Drawn in Slint: Rust builds an SVG path string per series and binds it to
  `Path.commands`; grid, axes and labels are `Rectangle`/`Text`. Range
  bucketing, downsampling and scaling are pure functions with unit tests.
- Interaction as in `InteractiveChartContainer`: range buttons 5h / 24h / 3d /
  7d / 30d, hover crosshair with a value tooltip, drag to zoom, double-click
  to reset. Overview draws one line per account in that account's avatar color.

## Command Log

Ported from `CommandRunner`, `CommandClassifier`, `TerminalLauncher`,
`RunningProcessRegistry`, `CommandLogStore`.

- One saved run command per account; triggers Manual, Auto (reset),
  Auto (empty), as on macOS.
- **Shell** (Settings): PowerShell 7 if installed, else Windows PowerShell;
  or cmd; or Git Bash. Started with the user's profile so functions and
  aliases resolve, like sourcing `~/.zshrc` on macOS.
- **Classifier:** the pure classification ports as is; the resolver uses
  `Get-Command` (PowerShell) or `type` (bash) to expand the leading token.
- **Non-interactive:** runs hidden with timeout, cancel and bounded output
  tail. Every child joins a Job Object with `KILL_ON_JOB_CLOSE`, so cancel or
  timeout kills the whole tree.
- **Interactive:** opens in Windows Terminal (`wt.exe new-tab …`), falling
  back to `conhost`.
- **Log view:** time, account, trigger, exit code, duration, output; stored in
  SQLite.
- **Active Claude Code account:** read from `%USERPROFILE%\.claude.json`
  (`oauthAccount.emailAddress`) for the sort tier and the card badge.

**Help:** the macOS help content, rewritten for Windows (extension, native
messaging, shells).

## Error handling

- Offline / API errors: keep last data, show staleness and the error.
- Expired keys: source-specific hint (see Data flow).
- Extension: badge and popup explain a missing host or a bridge error.
- Single instance: a named mutex; a second launch brings the running
  instance's window forward and exits.
- Store writes: lock file + temp-and-rename.

## Testing

- **`core`:** `cargo test` on Windows in CI. XDG-dependent tests gated
  `cfg(unix)`; Windows paths and DPAPI round-trip have their own tests.
  Contract cases run on both OSes.
- **`app`:** pure logic only — chart geometry, colors (parity with macOS
  values), sorting, command classification, popover placement against the
  work area. No window is opened in tests.
- **`bridge`:** injected `Environment`; framing, add/update/dedupe, muted
  sources, pipe notify.
- **Extension:** `node --test` over the message handling with `chrome.*`
  mocked.

## Release

- `.github/workflows/release-windows.yml` on `windows-latest`, triggered by a
  release: builds and uploads `ClaudeDashboard-x64.msi` and
  `claude-dashboard-extension.zip`.
- MSI per user via `cargo-wix`, no admin: installs to
  `%LOCALAPPDATA%\Programs\ClaudeDashboard`, registers the native-messaging
  host, adds a Start Menu shortcut.
- Auto-update as macOS `UpdateService`: GitHub `releases/latest`, download the
  `.msi`, run `msiexec /i … /passive`, relaunch.
- `scripts/sync-version.sh` and `scripts/release.sh` bump
  `apps/windows/Cargo.toml` and `apps/windows/extension/manifest.json`.

## Licensing

Slint is used under its Royalty-free license, which requires attribution:
the About section shows Slint's `AboutSlint` widget. Repository code stays MIT.

## Out of scope

- Code signing (unsigned MSI triggers SmartScreen).
- winget / Scoop packages.
- Publishing the extension to the Chrome Web Store and Edge Add-ons (IDs are
  reserved in `allowed_origins` for later).
- Decrypting `v20` app-bound cookies.
- Windows 10-specific UI work beyond the solid-background fallback.
