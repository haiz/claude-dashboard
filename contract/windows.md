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
| Command log DB | `%LOCALAPPDATA%\claude-dashboard\command_logs.db` |
| Saved run commands | `%APPDATA%\claude-dashboard\run-commands.json` |
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

    { "autoRefreshSeconds": 60, "preferredScanBrowser": "chrome", "launchAtStartup": false }

Settings are **non-critical**: a missing or corrupt file yields defaults and never
blocks startup. `autoRefreshSeconds` is clamped to [30, 3600] (default 60).
Changes take effect without a restart.

## Launch at startup

Enabled by writing value `ClaudeDashboard` = the quoted exe path under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; removing the value disables it.

## Scan behaviour

The Add Account scan classifies each browser profile as Found (a decodable `v10`
session), AppBound (`v20` cookie: the UI says "use the extension") or NoSession.
AppBound profiles are never force-decoded. Deleting an extension-sourced account
from the Settings Accounts pane mutes its install (see the muted rule above).

## App reload pipe

The app serves a named pipe `\\.\pipe\claude-dashboard`; the bridge writes
`reload\n` after a successful store change so a running app refreshes. A missing
pipe means the app is not running and is not an error.

## App shell

- **Single instance:** the app holds the named mutex
  `Local\ClaudeDashboardSingleInstance` for its lifetime; a second launch fails
  to acquire it and exits.
- **Reload pipe consumer:** the app is the *reader* of `\\.\pipe\claude-dashboard`
  and refreshes on each `reload` line; the bridge (sub-project 1) is the writer.

## Command Log

Each account can have one saved shell command, run by hand from the Run Command
panel or automatically when the account's usage window resets. Every run is
recorded in the command log (`core::command_log`, table `command_logs`).

**Vocabulary.** Persisted raw values are fixed: trigger `manual=0`,
`autoReset=1`, `autoEmpty=2` (labels `Manual`, `Auto (reset)`, `Auto (empty)`);
status `exited=0`, `timedOut=1`, `cancelled=2`, `launchedInTerminal=3`,
`launchFailed=4` (labels `Exited`, `Timed out`, `Cancelled`, `In Terminal`,
`Launch failed`). A NULL or unknown status reads as `exited`. `autoEmpty` is
recorded vocabulary only: macOS defines it but never fires it, and Windows keeps
the value and label for log compatibility. Only `autoReset` is ever written.

**Retention.** The newest 500 rows by id are kept (older ids are deleted after
each insert; the cap is by id, not time). The output tail is at most 4096 bytes
of UTF-8, cut forward to a char boundary; an empty output is stored as NULL.
Timestamps are Unix seconds. The log is advisory: a read failure yields an empty
list. The session key never appears in a command, row, UI string or error.

**Auto-run rule.** A saved command is due when the account has usage and its 5h
or 7d window has no `resets_at` (the API drops it between one cycle ending and
the next starting; `core::auto_run::should_run_saved_command`). It fires once
per episode: the latch arms only for accounts that have a saved, non-blank
command (so a command saved mid-episode fires on the next refresh), and
re-arms when both windows report a reset again. Auto runs are always hidden
(non-interactive) and recorded as `autoReset`, even when the saved toggle says
"Open in Terminal": with stdin on NUL and the 60 s timeout a TUI cannot hang
the app.

**run-commands.json.**

    { "version": 1, "commands": { "<accountId>": { "command": "...", "openInTerminal": false } } }

A missing, corrupt or other-version file loads as empty. Saving a blank
(whitespace-only) command removes the entry. Entries are keyed by account id and
dropped when the account is deleted; a re-sync or key repair keeps the id so the
command survives it, while delete-and-re-add creates a new id so the command is
gone. There is no prune pass. Writes are atomic and serialised.

**Shell choice.** `settings.json` key `"shell"` is `"pwsh"`, `"powershell"`,
`"cmd"` or `"bash"` (Git Bash); absent means auto. Auto picks the first installed
of PowerShell 7 (`pwsh.exe` on `PATH`, else `%ProgramFiles%\PowerShell\7`),
Windows PowerShell, Command Prompt (`%ComSpec%`); Git Bash is never chosen
automatically (`%ProgramFiles%\Git\bin\bash.exe`, else
`%LOCALAPPDATA%\Programs\Git\bin\bash.exe`). A configured shell that is not
installed falls back to auto. Settings > General > Commands lists installed
shells only.

**Hidden-run invocations** (the user's profile is loaded):
PowerShell: `-NoLogo -NonInteractive -Command "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; <command>"`;
cmd: `/s /c` with the raw tail `"<command>"`; Git Bash:
`-c "source ~/.bashrc 2>/dev/null\n<command>"`. The resolver (5 s timeout)
expands the leading token for the classifier, only when it contains nothing but
`[A-Za-z0-9._/\\:-]`: PowerShell `Get-Command` (definition and source), cmd
`where <token>`, Git Bash `-ic "type <token>"`.

**Classifier.** `core::command_classifier` suggests Open in Terminal for `claude`
(unless print mode: `-p`, `--print`, `--output-format`), a fixed list of TUIs
(vim, nvim, nano, emacs, top, htop, btop, less, more, tmux, irb, lazygit, fzf,
...), and `ssh host` with no remote command (a quoted remote command counts as
a positional, so `ssh host "df -h"` is non-interactive). The verdict is a
default and the user's toggle wins; the panel debounces typing by 350 ms.

**Job Object contract.** Every hidden run is spawned suspended
(`CREATE_SUSPENDED | CREATE_NO_WINDOW`), assigned to a Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resumed, so no instruction runs
outside the job. stdin is NUL, cwd is `%USERPROFILE%`, stdout and stderr are
merged into the bounded tail. Timeout is 60 s (`timedOut`); cancel gives
`cancelled`; both terminate the whole job tree. When the shell exits the runner
closes the job, so anything the command left running in the background is ended
and the output pipes always reach EOF (macOS kills the tree only on timeout or
cancel; here the job makes it uniform). Exit code is recorded only for
`exited`. A spawn or job failure is `launchFailed` with the reason as output.

**Interactive launch.** "Open in Terminal" runs detached (no job, no wait, cwd
`%USERPROFILE%`) and logs `launchedInTerminal` with no exit code. Primary:
`%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe new-tab --title "Claude Dashboard" -- <shell> ...`
with every `;` escaped as `\;` (wt would otherwise treat it as a subcommand
separator); fallback when `wt.exe` is absent: `%SystemRoot%\System32\conhost.exe <shell> ...`.
The shell tail keeps the session open: PowerShell `-NoLogo -NoExit -Command <command>`;
cmd `/k "<command>"`, passed raw (not argv-quoted), with `;` escaped under wt;
Git Bash `-l -i -c "<command>\nexec bash -l -i"`.

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
  -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden`, `CREATE_NO_WINDOW |
  DETACHED_PROCESS`). Its script waits for the app's PID to exit (up to 30 s),
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
