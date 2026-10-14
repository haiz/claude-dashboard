# Ping replaces Run Command (macOS)

Date: 2026-10-14
Status: approved design, awaiting implementation plan
Scope: `apps/macos/ClaudeDashboard` and `apps/macos/Shared` only. Windows and Linux keep their
Run Command feature unchanged; `contract/windows.md` is untouched. A new contract note documents
the ping endpoint sequence for future ports, but no other platform implements it in this work.

## Goal

Start an account's 5h (and 7d) usage window without signing in and without switching Claude
Code to that account. A "ping" sends one tiny message from the account through claude.ai using
the session key the dashboard already holds. Success: one click on an account page starts its
window, and with Auto Ping on, every active account whose window is not running gets one started
on the next refresh.

Run Command (a saved shell command per account, run by hand or after a reset) existed mainly to
do this. Ping replaces it on macOS, along with its sheet, runner, card button and log.

## Feasibility (spike, 2026-10-14)

A throwaway probe on `shared.engineering@gotitapp.co` (5h at 0%, `five_hour.resets_at: null`):

| | 5h utilization | `five_hour.resets_at` |
|---|---|---|
| Before | 0.0 | `null` |
| After ping at 03:44 UTC | 0.0 | `2026-10-14T08:40:00Z` |

The three requests below succeeded with a plain HTTP client and a Chrome User-Agent; no
Cloudflare challenge. The window end is rounded down to a 10-minute boundary. Not verified
directly: that Claude Code reads the same window (the `/usage` endpoint the dashboard and Auto
Switch already rely on changed, which is the indirect evidence).

## Behaviour

### Ping sequence

All requests go to `APIBaseURL` with the account's `sessionKey` cookie and the same headers as
`UsageAPIService.makeRequest` (`anthropic-client-platform: web_claude_ai`, JSON content type).

1. `POST /api/organizations/{orgId}/chat_conversations`, body `{"uuid": <client UUID>, "name": ""}`.
   Expect 201.
2. `POST /api/organizations/{orgId}/chat_conversations/{uuid}/completion`, body
   `{"prompt": "Reply with just: ok", "timezone": TimeZone.current.identifier,
   "attachments": [], "files": [], "rendering_mode": "messages"}`. The response is SSE. Read it
   until `message_stop` or end of stream, with a 60 s timeout. Seeing a `message_start` event
   means the ping succeeded. The stream is not cancelled early: whether an aborted completion
   still counts as usage is unverified.
3. `DELETE /api/organizations/{orgId}/chat_conversations/{uuid}`, best effort. It also runs when
   step 2 fails after step 1 succeeded, so no stray chat is left behind.

No `model` field is sent; the account's default model is used. `Set-Cookie` on these responses
is ignored: the refresh that follows every ping already handles session key rotation.

### Outcomes

| Situation | `PingOutcome` |
|---|---|
| `message_start` seen (DELETE result does not matter; a failed DELETE sets `detail`) | `.ok` |
| 401, or 403 with a session error | `.expired` |
| 403 with a `cf-mitigated` header or an HTML body | `.blocked` |
| 429, timeout, network error, no `message_start`, any other status | `.failed(detail)` |

`.expired` does not change account status directly; the refresh after the ping marks the account
`.expired` through the existing path.

### Manual ping

The account page (`AccountPane`) replaces the "Run a command for this account" row with
"Start the 5h window now" and a **Ping** button, in the same position (between "Switch here after
5h reset" and "Re-sync").

- Always visible. Disabled when the account is not `.active` or a ping for it is in flight
  (spinner shown while in flight).
- One click pings. No sheet, no confirmation.
- Under the button, a caption shows the account's latest ping from the Ping Log, for example
  "Pinged 10:44" or "Ping failed: session expired". It survives restarts because it reads the log.
- After the ping, the account is refreshed (`refreshAll(only: [id])`) so `resets_at` shows at once.
- Manual ping works whether Auto Ping is on or off and has no cooldown.

### Auto Ping

Settings > General gets an **Auto Ping** section with one toggle, "Ping when a usage window is not
running", key `autoPingEnabled`, **default off**. It applies to every account; there is no
per-account setting. Caption: "Starts a new window for every active account whose 5h or 7d
window is not running. Each ping sends one short message from that account."

On every refresh (interval, manual, or the existing 5 s `resetMonitorTask` firing after a reset
time passes), an account is auto-pinged when all hold:

- `autoPingEnabled` is true;
- `account.status == .active`;
- this refresh fetched its usage successfully (a failed fetch never pings: the window state is
  unknown);
- `five_hour.resets_at == nil` **or** `seven_day.resets_at == nil`;
- it is not latched, not in flight, and not in the failure cooldown.

This deliberately covers accounts that have been idle for a long time, not only accounts whose
window just reset: the rule is "a successfully refreshed account whose window is not running
needs a ping". Turning the toggle on triggers one refresh so idle accounts are pinged without
waiting for the interval.

De-duplication, all in memory:

- **Latch.** After an auto ping is sent, the account is latched. The latch is cleared only when a
  refresh shows both `five_hour.resets_at` and `seven_day.resets_at` non-nil. This stops repeat
  pings while the API still reports `null` for a few seconds after a ping.
- **In flight.** An account never has two pings at once; a manual click during an auto ping (or
  the reverse) is skipped.
- **Failure cooldown.** A failed auto ping (any outcome except `.ok`) keeps the latch and sets a
  10-minute cooldown; after it, the next refresh may try again. The old auto-run never retried.

### Settings and cards removed

- The terminal icon (Run Command) is removed from `AccountCard`, on the Dashboard grid and in the
  menu bar popover. No ping button is added to cards.
- The "Cards" section ("Show Run Command button on cards", key `menuBarShowRunCommand`) is removed
  from Settings > General.

### Ping Log

The sidebar's Tools item "Command Log" becomes **Ping Log**.

`PingLogStore` (SQLite, `ping_logs.db` in the directory `command_logs.db` uses today):

| Column | Type | Meaning |
|---|---|---|
| `id` | INTEGER PK | row order |
| `account_id` | TEXT | `Account.id` |
| `trigger` | INTEGER | 0 manual, 1 auto |
| `started` | REAL | start time |
| `finished` | REAL | end time |
| `outcome` | INTEGER | 0 ok, 1 expired, 2 blocked, 3 failed |
| `detail` | TEXT NULL | error or note |

Raw values of `trigger` and `outcome` are fixed and never renumbered. One row per ping, written
when it finishes. The newest 500 rows are kept. API: `insert`, `fetch(limit:)`,
`latest(for:)`, `clear()`; `init(dbPath:maxEntries:)` so tests use a temp file.

`PingLogView` lists time, account name (via `AccountStore`; "Removed account" when gone),
Manual/Auto, outcome and detail, with the existing Clear button. No output or exit-code columns.

### Migration

`PingMigration.run(defaults:fileManager:logsDirectory:)` runs at launch. Every step is idempotent,
so no "migrated" flag is stored:

- delete `command_logs.db` and its `-wal` / `-shm` files;
- delete every `runCommand_*` and `runCommandTerminal_*` key and `menuBarShowRunCommand` from
  `AppDefaults.shared`;
- leave every other key alone. Nothing is carried over to Auto Ping; it starts off.

`AccountStore` drops its `RunCommandSettings.remove` / `prune` calls. Ping stores nothing per
account in UserDefaults, so deleting an account needs no extra cleanup; its log rows age out.

## Components

| Unit | Location | Responsibility | Depends on |
|---|---|---|---|
| `PingService` | `Shared/PingService.swift` | The three requests and outcome mapping. `init(session: URLSession)`, `ping(orgId:sessionKey:) async -> PingResult` (`outcome`, `detail`) | `URLSession`, `APIBaseURL` |
| `PingController` | `ClaudeDashboard/Services/Ping/PingController.swift` | `@MainActor`. `shouldAutoPing(for:)`, latch, in-flight set, cooldown, `pingNow(account:trigger:)`, publishes per-account in-flight state and latest result, writes the log | `PingService` (protocol), `PingLogStore`, clock, a refresh callback |
| `PingLogStore` | `ClaudeDashboard/Services/Ping/PingLogStore.swift` | SQLite persistence | SQLite3 |
| `PingMigration` | `ClaudeDashboard/Services/Ping/PingMigration.swift` | Launch cleanup | `UserDefaults`, `FileManager` |
| `PingLogView` / `PingLogViewModel` | `Views/`, `ViewModels/` | Ping Log pane | `PingLogStore`, `AccountStore` |

`DashboardViewModel.refreshAll` calls the controller where it evaluates `shouldRunSavedCommand`
today; `pingedAccounts` moves into the controller as the latch. `AutoSwitch` is unchanged.

Removed from macOS: `CommandRunner`, `CommandClassifier`, `TerminalLauncher`, `ProcessTree`,
`RunningProcessRegistry`, `RunCommandSheet`, `RunCommandSettings`, `CommandLogModels`,
`CommandLogStore`, `CommandLogView`, `CommandLogViewModel`, their tests, and the quit-time process
reaping in `ClaudeDashboardApp`.

## Testing

TDD in `ClaudeDashboardTests`; no real network.

- `PingServiceTests` (`MockURLProtocol`): request order, paths, cookie and bodies; SSE with
  `message_start` gives `.ok`; failed DELETE still `.ok` with `detail`; 401 `.expired`; 403 with
  `cf-mitigated` `.blocked`; 429 `.failed`; DELETE sent after a failed completion.
- `PingControllerTests` (fake service, injected clock): `shouldAutoPing` for 5h nil, 7d nil, both
  set, usage nil; toggle off; non-active account; latch holds while the API still reports nil and
  clears when both are set; 10-minute cooldown then retry; in-flight blocks a second ping; one log
  row per ping.
- `PingLogStoreTests`: insert, fetch, `latest(for:)`, prune at 500, clear, on a temp DB.
- `PingMigrationTests`: removes the old DB and its `-wal`/`-shm`, removes the three key families,
  keeps other keys, second run is a no-op.
- `DashboardViewModelTests`: saved-command tests are replaced by "a refresh triggers auto ping".
  The harness must stub `BrowserCookieService` too (the view model is not isolated otherwise).
- Manual check in the real app: Ping on an account with `resets_at: null` shows a reset time and a
  Ping Log row; turning Auto Ping on pings each such account once.

## Documentation

- `CLAUDE.md`: add a **Ping** entry under Services (`Services/Ping/`, `Shared/PingService.swift`);
  remove the macOS Run Command / Command Log mentions; sidebar Tools becomes Ping Log, Help.
- `contract/README.md`: a short "Ping" section with the three-request sequence and outcome
  mapping, flagged as an undocumented claude.ai endpoint that may change.
- `HelpView`: replace the Run Command paragraph with Ping and Auto Ping.

## Risks

- The completion endpoint is undocumented; its body, headers or Cloudflare rules can change. The
  `.blocked` / `.failed` outcomes and the Ping Log make that visible instead of silent.
- Automating claude.ai chat is a grey area of the terms of use. It touches no OAuth token and no
  Claude Code state, which keeps it far from the token-sharing restriction.
- Each ping costs one short message from the account's quota on its default model.
