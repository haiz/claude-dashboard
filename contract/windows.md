# Windows behaviour

Platform-scoped, for the Windows app (`apps/windows/`) and its native-messaging
bridge. Account **business rules** — plan detection, dedupe, org selection,
burn rate, the Fable window — are unchanged and stay governed by the other
`contract/` files; they now run on Windows too (the Swift-mirroring `core`
crate's tests execute under `cargo test` on `windows-latest`). This file covers
only what is Windows-specific.

## Data paths

| File | Location |
|---|---|
| Accounts store | `%APPDATA%\claude-dashboard\accounts.json` |
| Usage log DB | `%LOCALAPPDATA%\claude-dashboard\usage_logs.db` |
| Extension sources | `%APPDATA%\claude-dashboard\extension-sources.json` (beside the store) |
| Store write lock | `%APPDATA%\claude-dashboard\accounts.json.lock` |

Two processes write `accounts.json` on Windows (the app and the bridge). A
writer holds an exclusive lock on `accounts.json.lock` across its whole
read-modify-write, then writes a temp file and renames it over the store.

## At-rest session-key protection

Session keys in the store are sealed with the Windows Data Protection API
(`CryptProtectData`, current-user scope) and base64-encoded — the same one
base64-string wire shape as the Unix scheme, but **not** byte-compatible: a
store is useless on another machine or OS, and `sessionKey` was never portable
(see `account-schema.md`, "sessionKey is not portable").

## Cookie formats

Chromium cookies on Windows carry a 3-byte version tag:

- `v10` / `v11`: AES-256-GCM, body = `nonce(12) || ciphertext || tag`, keyed by
  `Local State`'s `os_crypt.encrypted_key` (base64, `"DPAPI"` prefix, then a
  DPAPI blob). Decoded by `core::cookie::win::decode_value`.
- `v20`: app-bound encryption (Chrome 127+), whose key is bound to the browser
  through a SYSTEM service. **Never opened.** It reports
  `CookieError::AppBoundEncrypted`, and the app routes the user to the browser
  extension instead of scanning.

A schema-version >= 24 plaintext carries a `SHA256(host_key)` prefix, stripped
exactly as on the other platforms.

## Native messaging

Host name: `com.claude_dashboard.bridge`. The browser launches a fresh host
process per message; the host reads one message, acts, replies, and exits.

Framing: a 32-bit little-endian length followed by that many bytes of UTF-8
JSON (the Chrome native-messaging convention).

**Message in** (extension → host):

    { "type": "sessionKey", "installId": "<uuid>", "browser": "chrome|edge|brave",
      "sessionKey": "<key>" }

**Reply out** (host → extension):

    { "ok": true, "email": "<email or null>" }
    { "ok": false, "error": "<code>", "message": "<human text>" }

Error codes: `bad_message` (unparseable, wrong type, or missing install id),
`no_key` (blank session key), `muted` (this install's account was deleted),
`rejected` (the session key was not accepted by `/api/account`), `no_chat_org`
(the account has no organization with chat access), `store` (an account- or
sources-store read/write failed).

The session key never appears in a reply, a log line, or any error message.

Processing order (the bridge): parse and validate (no I/O) → muted check
(before the network) → fetch `/api/account` then `/api/organizations` (never
under the lock) → under the store lock: reload sources, re-check muted, load
accounts, apply the key (add or repair, via `core::key_intake`), save accounts,
bind the install, save sources → after the lock: notify the app.

## Registration

The host manifest (`com.claude_dashboard.bridge.json`) sits next to the bridge
exe and is registered under the per-user key each browser reads. The MSI ships
a static manifest (`apps/windows/app/wix/com.claude_dashboard.bridge.json`)
whose `path` is the relative `claude-dashboard-bridge.exe`; Chromium resolves a
relative `path` against the manifest's directory on Windows. It registers the
same two HKCU keys, each defaulting to the installed manifest's absolute path
(`[INSTALLDIR]com.claude_dashboard.bridge.json`):

- `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.claude_dashboard.bridge`
  (Chrome, Brave, Arc)
- `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.claude_dashboard.bridge`
  (Edge)

`allowed_origins` lists the extension's fixed id
(`chrome-extension://cadpjcfajhlgdaipepkdojcehfmkkedh/`), which is fixed by the
`key` field in the extension manifest. `apps/windows/scripts/register-dev-host.ps1`
is for dev builds only: it writes a manifest with an absolute `path` and the same
registry keys.

## extension-sources.json

    {
      "bindings": { "<installId>": { "accountId": "<id>", "browser": "chrome" } },
      "muted": ["<installId>", ...]
    }

`bindings` records which extension install feeds which account, so the app can
tell an extension-sourced account from a hand-pasted one. **Muted rule:**
deleting an extension-sourced account adds its `installId` to `muted`; the host
then ignores that install's keys (reply `muted`) so a deleted account is not
silently re-added. Settings offers an unmute. This file is kept out of
`accounts.json` so the cross-platform account schema stays unchanged;
extension-sourced accounts are stored with `source: "manual"`.

## settings.json

`%APPDATA%\claude-dashboard\settings.json`:

    { "autoRefreshEnabled": true, "autoRefreshMinutes": 5, "preferredScanBrowser": "chrome", "launchAtStartup": false }

Settings are **non-critical**: a missing or corrupt file yields defaults and never
blocks startup. `autoRefreshEnabled` defaults to true and `autoRefreshMinutes` to 5 (as on macOS),
clamped to [1, 60]; when disabled the app refreshes only on demand. A legacy
`autoRefreshSeconds` (no `autoRefreshMinutes` present) migrates on load to
`round(seconds / 60)` clamped to [1, 60] (0 means 5) and is not written back. The
removed `shell` key is dropped the same way.
Changes take effect without a restart. An Auto Refresh change only re-arms the
refresh timer with the new interval (as macOS reschedules its timer); it never
starts a fetch. Writes from the Settings pane run one at a time, in click order.
`hasLaunchedBefore` (default false) is set on the first launch and survives Reset
All Settings.

## Launch at startup

Enabled by writing value `ClaudeDashboard` = the quoted exe path under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; removing the value disables it.

## Scan behaviour

The Add Account scan classifies each browser profile as Found (a decodable `v10`
session), AppBound (`v20` cookie: the UI says "use the extension") or NoSession.
AppBound profiles are never force-decoded. Deleting an extension-sourced account
from the Settings Accounts pane mutes its install (see the muted rule above).

The scan follows macOS `SetupView`. It reads one browser at a time, picked from
the installed ones (those whose `User Data` directory exists) and saved as
`preferredScanBrowser` in `settings.json`. With two or more browsers and no
saved choice that is still installed, the tab asks which one before it reads
any cookie; otherwise it scans when the tab first opens. Each Found profile's
key is checked with `/api/account`, and its org is resolved as on macOS. Any
account already stored (`contract/cases/dedupe.json`) is skipped. The scan
offers the rest, showing the email (else the profile name), the plan, and
`<Browser>: <Google email, else profile dir>`. When nothing is offered, it
lists each skipped profile in natural order under "Nothing new to add. What
the scan found:". The reasons are: already added as X, signed out (HTTP
401/403), no organization with chat access, could not be read, and, Windows
only, app-bound (use the Browser extension tab). Add Selected stores the picked
accounts as scanned accounts (`source: browser`, `chromeProfilePath` = the
profile dir, `chromeProfileName` = the Google email, else the profile name). It
re-checks dedupe, refreshes and closes the wizard. A pasted key that is saved
closes the wizard too, unless the account has no organization with chat
access: then the warning stays on screen.

Scan roots, under `%LOCALAPPDATA%`: `Google\Chrome\User Data`,
`Google\Chrome Dev\User Data` (shown as "Chrome Dev", stored as `"chromeDev"`),
`Microsoft\Edge\User Data` and `BraveSoftware\Brave-Browser\User Data`. Arc is
reached only through the extension.

Downgrade: a Windows build from before Chrome Dev support has no `chromeDev`
browser, so it fails to decode a `"chromeDev"` record (the same hazard
`contract/account-schema.md` describes for macOS). Once a Chrome Dev account is
added, do not run such a build.

## Account kinds

Windows has three kinds of account (macOS has only scanned and pasted). Labels
and hints come from `core::account_kind`:

| Kind | How to tell | Settings > Accounts label | Expired-card hint | Re-sync |
|---|---|---|---|---|
| Scanned | `source` is a browser and `chromeProfilePath` is set | the profile name (else path) | `Open <browser> profile "<profile>" and login to claude.ai, then re-sync.` | re-reads the key from that `v10` profile with the macOS `resyncCore` rules: stop if the profile now holds a different account (by `account_uuid`), else save the key, set `active` and re-resolve the org (the `lastActiveOrg` cookie, ignored when empty, else the stored org, as the org-selection preference) |
| Extension | `source: manual` with a binding in `extension-sources.json` | `Browser extension (<browser>)` | `Open <browser> and sign in to claude.ai; the extension sends the new key.` | reads nothing; refreshes usage |
| Pasted key | `source: manual`, no binding | `Pasted key` | `This key was pasted by hand. Add it again from Settings › Accounts, Add Account, "Paste a session key instead".` | fails with "This account's session key was pasted by hand. Paste a new one to re-sync it." |

**Re-sync All** re-syncs every account by its kind, then refreshes the accounts
that re-synced in a single pass (macOS `refreshAll(only:)`); a single card's
Re-sync refreshes that account only. Its "Re-syncing… (n/total)" progress counts
scanned accounts only; per-account failures show in that card's error line after
the refresh.

## Refresh

The app's refresh follows macOS `refreshAll`: an HTTP `401`/`403` marks the
account `expired` (stored, and expired accounts are no longer polled until a
re-sync); other failures retry 3 times 1 s apart and show on the card while it
keeps its last-good usage and sort position; a successful fetch sets `active`,
`lastSynced`, the plan from `/api/organizations` and a missing `accountUuid` /
`email` from `/api/account`. The Auto Refresh tick keeps its own schedule (only a
settings change restarts it); pinning re-sorts without a fetch; countdowns and the
tray tooltip move every second; a 5h/7d reset that passed more than 10 s ago
triggers one refresh. Usage history older than 90 days is pruned at launch.

## App reload pipe

The app serves a named pipe `\\.\pipe\claude-dashboard`; the bridge writes
`reload\n` after a successful store change so a running app refreshes. A missing
pipe means the app is not running and is not an error.

## App shell

- **Single instance:** the app holds the named mutex
  `Local\ClaudeDashboardSingleInstance` for its lifetime; a second launch fails
  to acquire it and exits.
- **Main window:** as on macOS it opens by itself only on the first launch
  (`hasLaunchedBefore`); later launches, Launch at startup included, start in the
  tray. Showing it with no accounts opens the Add Account wizard. Closing it hides
  it and returns it to the Dashboard. The tray menu's Quit asks first, with the
  flyout's Quit prompt text.
- **Reload pipe consumer:** the app is the *reader* of `\\.\pipe\claude-dashboard`
  and refreshes on each `reload` line; the bridge (sub-project 1) is the writer.

## Launch cleanup

Run Command and the Command Log were removed (as on macOS). At startup the app
deletes the files they left behind, ignoring every error:
`%LOCALAPPDATA%\claude-dashboard\command_logs.db` (with its `-wal` and `-shm`)
and `%APPDATA%\claude-dashboard\run-commands.json`
(`migration::run_launch_cleanup`). It does not run under
`CLAUDE_DASHBOARD_FAKE_ROWS` or `CLAUDE_DASHBOARD_SMOKE`.

## Claude Code badge

**Active Claude Code account.** `%USERPROFILE%\.claude.json` ->
`oauthAccount.emailAddress` (trimmed; missing, unparseable or blank is none) is
matched exactly against each account's email. A match shows the green dot
(Claude Code badge). It adds a sort tier after pinned accounts and before burn
rate, applied only when no account is pinned; the badge shows regardless.

## Installer

`apps/windows/scripts/build-msi.ps1` builds the release workspace, then runs
`cargo wix` (WiX 3.14, `apps/windows/app/wix/main.wxs`) to produce
`apps/windows/target/wix/ClaudeDashboard-x64.msi`. The version comes from the
Cargo workspace version, which `scripts/sync-version.sh` keeps equal to `/VERSION`.

- **Scope:** per user, x64 (`InstallScope="perUser"`, `InstallPrivileges="limited"`);
  no admin prompt. Everything lives under the user's profile and HKCU.
- **Install dir:** `%LOCALAPPDATA%\Programs\ClaudeDashboard\`, holding
  `claude-dashboard.exe`, `claude-dashboard-bridge.exe` and
  `com.claude_dashboard.bridge.json`.
- **Registry:** the two native-messaging keys from "Registration", plus the
  marker values `HKCU\Software\ClaudeDashboard` `Installed` and `StartMenuShortcut`.
- **Start Menu:** a per-user shortcut "Claude Dashboard" targeting the installed exe.
- **UpgradeCode** `173B2BBC-0CBB-472C-B3BF-E88EE21C4D58`, fixed forever. The two
  component GUIDs (`AppFiles`, `NativeMessagingHost`) are fixed too and must
  never change, or upgrades orphan files and keys.
- **Upgrade:** `MajorUpgrade` (scheduled after `InstallInitialize`) replaces the
  old version in place. A downgrade is refused with "A newer version of Claude
  Dashboard is already installed."
- **Uninstall:** removes the files, the install and Programs directories (when
  empty), the Start Menu shortcut, both native-messaging keys and the
  `ClaudeDashboard` value under `HKCU\...\CurrentVersion\Run` (the app's Launch at
  startup). The `Run` value is dropped by a deferred `reg.exe delete` custom
  action, conditioned on `REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE` so an upgrade
  keeps the setting, and `Return="ignore"` because the value may be absent.
- **User data is never touched:** the installer does not reference
  `%APPDATA%\claude-dashboard` or `%LOCALAPPDATA%\claude-dashboard`, so accounts,
  settings and logs survive both upgrade and uninstall.
- **One-liner:** `/install.ps1` (published URL, must stay at the repo root;
  `irm .../install.ps1 | iex`) resolves the latest release's assets by exact
  name, stops a running `claude-dashboard.exe`, runs `msiexec /i <msi> /passive
  /norestart` (exit 0 or 3010 is success), unpacks the extension zip to
  `%LOCALAPPDATA%\claude-dashboard\extension` (replacing that folder only) and
  starts the app. A release without the MSI is an error; one without the zip
  only skips the extension. It never calls `exit` (under `iex` that closes the
  user's shell), stays ASCII-only for Windows PowerShell 5.1, and loads
  definitions only when `CLAUDE_DASHBOARD_INSTALL_TEST` is set.
  `apps/windows/scripts/test-install-ps1.ps1` checks it.
- **Artifacts:** `ClaudeDashboard-x64.msi` and `claude-dashboard-extension.zip`.
  The zip is built by `apps/windows/scripts/pack-extension.ps1` with .NET
  `ZipArchive`, using `/` entry names and an allowlist of seven files
  (`manifest.json`, `background.js`, `popup.html`, `popup.js`, `lib/extension-id.js`,
  `lib/status.js`, `lib/sync.js`), so a local `extension-private.pem` can never be
  packed. `test-pack-extension.ps1` checks it.

## Updates

Release checks and decisions live in `core::update` (shared with Linux); the
download and install live in the app (`apps/windows/app/src/updater.rs`).

- **Endpoint:** `GET https://api.github.com/repos/haiz/claude-dashboard/releases/latest`
  with `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`
  and `User-Agent: claude-dashboard/<version>`; 20 s timeout. A non-200 or
  transport failure is an error.
- **Version:** the tag is `vX.Y.Z`; one leading `v` is stripped. `is_newer`
  compares dot-separated components numerically (missing component = 0,
  non-numeric components dropped), so `1.18.10 > 1.18.9` and an equal or lower
  version is never offered.
- **Ignored:** a `draft` or `prerelease` release yields no update.
- **Asset:** the release must carry `ClaudeDashboard-x64.msi`; a newer release
  without it is an error ("The release has no Windows installer.").
- **Schedule:** the background loop wakes hourly and checks when the last check
  is at least 24 h old (`CHECK_INTERVAL_S` = 86400), never checked, or the clock
  moved backwards. The stamp is written before the request.
- **Settings:** `settings.json` keys `autoUpdate` (bool, default `true`) and
  `lastAutoUpdateCheck` (Unix seconds, omitted until the first check).
- **Installed copy only:** auto-update runs only when the exe is
  `%LOCALAPPDATA%\Programs\ClaudeDashboard\claude-dashboard.exe` (compared
  case-insensitively, `/` read as `\`). A dev build may check but shows
  "Development build — install the MSI to update" and never downloads or runs
  `msiexec`. Auto-update also requires `autoUpdate` on and `DISABLE_AUTOUPDATER`
  not equal to `1`. The loop does not run under `CLAUDE_DASHBOARD_SMOKE` or
  `CLAUDE_DASHBOARD_FAKE_ROWS`.
- **Auto vs manual:** when the daily check finds a newer release the app
  downloads it, starts the relauncher and quits. Settings > General > Updates
  "Check for Updates" only reports the result and offers an Install button.
- **Download checks:** the version in the release must match `[0-9A-Za-z.-]+`
  (it names a temp file). The MSI is streamed to
  `%TEMP%\ClaudeDashboard-update-<version>-<uuid>.msi` with a 300 s timeout and
  must be at most 200 MiB and start with the OLE compound-file magic
  `D0 CF 11 E0 A1 B1 1A E1`. Otherwise the file is deleted and the update fails
  ("The update is too large." / "The downloaded update is not a Windows
  installer.").
- **Relauncher:** a hidden, detached `powershell.exe` (`-NoProfile
  -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden`, `CREATE_NO_WINDOW`
  only — not `DETACHED_PROCESS`: a detached `powershell.exe` exits without running
  `-Command`; the child is in no job, so it outlives the app). Its script waits for the app's PID to exit (up to 30 s),
  runs `msiexec /i "<msi>" /passive /norestart` and waits for it, then starts the
  installed exe. Waiting first means Windows Installer never meets a locked exe
  and the single-instance mutex is free on relaunch. Each path is a PowerShell
  single-quoted literal with `'` doubled, and also U+2018, U+2019, U+201A and
  U+201B, which PowerShell treats as single quotes. The script carries no secrets.

## Release

`.github/workflows/release-windows.yml` runs on `windows-latest` after
`scripts/release.sh` publishes a release (`release: types: [published]`), or by
`workflow_dispatch` with an existing `tag` as the retry path. It checks out the
resolved tag, installs the pinned toolchain, runs clippy and tests with
`--locked`, installs WiX 3.14 from the pinned zip (SHA-256 verified) and
`cargo-wix` 0.3.9 (`--locked --version`), builds the MSI, packs the extension and
runs `gh release upload ... --clobber` for `ClaudeDashboard-x64.msi` and
`claude-dashboard-extension.zip`.

Build input: `CLAUDE_DASHBOARD_RELEASE_DATE` (`yyyy-MM-dd`), the tag's commit
date (`%cs`), set before the MSI build. `app/build.rs` bakes it into the binary
for the About header ("Oct 17, 2026"); unset or empty (local builds, or no git
output) means no date is shown.

Guardrails, the same five as `release-linux.yml`: published-release trigger only
(never a tag push); upload only (exactly one `gh release` line, which is `upload`;
no `gh api`; the only `uses:` is `actions/checkout`); never name the macOS
artifacts `ClaudeDashboard.app.zip` or `claude-dashboard-cli.tar.gz`; never write
release notes (`release.sh` does); never commit or push. Windows-specific: the
exact MSI and zip upload paths, the WiX SHA-256 and the `cargo-wix` pin must
appear, and checkout must be pinned to the resolved tag.
`scripts/test-release-workflow.sh` enforces all of this on both workflows against
a comment-stripped copy of each file.

## Manual smoke test

End-to-end extension → bridge → store is verified by hand (the automated
`roundtrip` test covers framing and the rejected path; the happy path is
covered by the handler's unit tests):

1. Build the bridge, then run `apps/windows/scripts/register-dev-host.ps1`
   with the built exe path and the fixed extension id.
2. Load `apps/windows/extension/` as an unpacked extension.
3. Open claude.ai logged in. The extension popup should show
   "Synced <email>", and the account should appear in `accounts.json`.

### UI smoke test

`CLAUDE_DASHBOARD_FAKE_ROWS=1 cargo run -p claude-dashboard` (stop any installed copy first; it holds the
single-instance mutex). Check each pane:

- **Dashboard:** five cards: alice (pinned, Pro), bob (Max 5x, green Claude Code dot, Fable ring at 40%),
  carol (5h at 100%), dave (expired, pasted-key hint), erin (expired, scanned-browser hint).
- **Account page (bob):** summary gauges, History chart with a reset inside the last 24 h and a limited
  point, Reset Cycles list, Pin / Re-sync / Remove with inline confirmation.
- **Overview:** one line per account with data, `(no data)` chips for alice, dave and erin, hover card,
  pan and window chips.
- **Settings > Accounts:** Add Account wizard (extension / scan / paste), Re-sync All progress `(n/3)` (alice, carol and erin
  are the scanned rows),
  Delete with Confirm / Cancel, kind labels (Default, Browser extension (Microsoft Edge), Pasted key, Work).
- **Settings > General:** About header, info tips on Enable auto refresh and Interval, interval stepper,
  Launch at startup, Updates card, Reset All Settings with confirmation (accounts, logs and launch at
  startup kept). The stepper and Reset write the real `settings.json`, even under FAKE_ROWS.
- **Help:** the seven topic chips and their cards.
- **Tray flyout:** left-click the tray icon; cards fit, Quit confirmation.
