# Helper CLI

The privileged helper binary (`apps/macos/Helper/main.swift`) exposes exactly
four subcommands: `decrypt`, `usage`, `sync`, `add-key`. Their externally
observable behaviour — input/output shape, exit codes, stderr text — is
contract. How each subcommand is invoked from a wrapper script, and how
`sync` persists accounts, is platform detail.

Dispatch: no subcommand at all (`apps/macos/Helper/main.swift:5-17`) prints a
usage banner to stderr and exits 1; an unrecognized subcommand
(`apps/macos/Helper/main.swift:30-32`, the `default:` case of the switch at
lines 21-33) prints `Unknown command: <command>\n` to stderr and exits 1.
This is not itself a contract requirement — only the four named
subcommands' behaviour below is.

## `decrypt`

Source: `apps/macos/Helper/DecryptCommand.swift`.

Loads the persisted account list (`HelperAccountStore.loadAccounts()`, line
15). Output is a **six-field projection**, not a serialized `Account`. The
`DecryptedAccount` shape (lines 5-12):

| Field | Type | Source |
|---|---|---|
| `name` | string | `account.name` |
| `email` | string or null | `account.email` |
| `orgId` | string or null | `account.orgId` |
| `sessionKey` | string or null | see below |
| `plan` | string | `account.plan.rawValue` (a wire value from `account-schema.md`) |
| `status` | string | `account.status.rawValue` |

**Inclusion filter** (lines 22-24): an account is included only when
`account.status == .active` **and** `account.orgId != nil`. Note that
`sessionKey` is *not* part of this filter — an included account can still
have `sessionKey: null` in the output if the stored account itself has no
session key.

**`sessionKey` value** (lines 26-29): if the stored account has a
`sessionKey`, the output value is `CryptoService.decrypt(encrypted) ??
encrypted` — i.e. the plaintext session key on successful decryption, but
**silently falls back to the still-encrypted ciphertext string** if
decryption fails (wrong machine, corrupted value, etc.). There is no error
path for a failed decrypt; the caller receives ciphertext masquerading as a
plain value with no signal that decryption failed.

**Encoding** (lines 46-47): `JSONEncoder` with `[.prettyPrinted,
.sortedKeys]` — output keys are alphabetical (`email`, `name`, `orgId`,
`plan`, `sessionKey`, `status`), not struct-declaration order.

**Failure paths**, all three to stderr with exit code 1:
1. No accounts stored at all (line 17-20): stderr is exactly
   `No accounts found. Run: claude-dashboard-cli sync\n`.
2. Accounts stored but the inclusion filter above produces an empty list
   (line 41-44): stderr is exactly `No active accounts with session keys
   found.\n`. Note this message names "session keys" but the actual gate is
   `status == active && orgId != nil` (see above) — the wording is a
   misnomer in the source; a Rust port must match this stderr text
   verbatim, not the more accurate description of the filter.
3. JSON encoding itself fails (line 48-52 — `guard let data = try?
   encoder.encode(decrypted), let json = String(data: data, encoding:
   .utf8) else { ... }`): stderr is exactly `Failed to encode accounts.\n`.
   This path exists for completeness of the contract even though it is not
   expected to trigger in practice — `DecryptedAccount` only holds `String`
   and `String?` fields, which `JSONEncoder` does not fail to encode.

Success: exit 0, pretty-printed JSON array printed to stdout (line 54).

## `usage <orgId> <sessionKey>`

Source: `apps/macos/Helper/UsageCommand.swift`. This is a **passthrough** —
it does not parse or reshape the upstream payload in any way.

Requires exactly two positional arguments (lines 6-9); fewer prints
`Usage: claude-dashboard-helper usage <orgId> <sessionKey>\n` to stderr and
exits 1.

Request (lines 14-24): `GET
https://claude.ai/api/organizations/<orgId>/usage` with headers:
- `accept: */*`
- `content-type: application/json`
- `anthropic-client-platform: web_claude_ai`
- `Cookie: sessionKey=<sessionKey>`

These are the same four headers `UsageAPIService.makeRequest`
(`apps/macos/Shared/UsageAPIService.swift:125-133`) sends from the main app
— the helper does not add or omit anything.

Timeout: 15 seconds (line 41, `semaphore.wait(timeout: .now() + 15)`).

Exit 1, with the reason on stderr, on any of:
- Invalid `orgId` (line 14-17 — `URL(string:
  "https://claude.ai/api/organizations/\(orgId)/usage")` returns `nil`,
  e.g. an `orgId` containing characters that are illegal in a URL path):
  `Invalid orgId.\n`.
- Timeout (line 41-45): `Request timed out.\n`.
- Network error (line 47-50): `Network error: <localizedDescription>\n`.
- Non-2xx HTTP status (line 52-55): `HTTP <status>\n`.
- Empty or non-UTF8 body (line 57-61): `Empty response.\n`.

Success: exit 0, the upstream response body printed to stdout **byte-for-byte
unchanged** (line 63) — whatever JSON shape claude.ai returned that day,
including fields the Swift `UsageData` decoder does not know about.

### Why the passthrough shape matters

`cli/claude-dashboard-cli` (the bash wrapper) does not call the API
directly — it shells out to this helper and reads the printed body with
`jq`, including this lookup (`cli/claude-dashboard-cli:322-324`):

```sh
pct_sonnet=$(echo "$usage_json" | jq -r '.seven_day_sonnet.utilization // empty' 2>/dev/null || echo "")
reset_sonnet_raw=$(echo "$usage_json" | jq -r '.seven_day_sonnet.resets_at // ""' 2>/dev/null || echo "")
```

`seven_day_sonnet` is a field the current API no longer returns (see
`README.md`'s "The Fable window" section) — the third usage window has been
`fable` for some time. The `// empty` guard means this lookup produces an
empty string rather than an error when the field is absent, so the bash CLI
degrades silently: it just renders a blank Sonnet column instead of failing.
This is precisely why the `usage` subcommand's contract is "print the
upstream body verbatim": if a Rust reimplementation of `usage` decoded the
payload into a typed struct and re-serialized only the fields it knew about,
`seven_day_sonnet`-style dead lookups would keep silently returning empty —
but so would every *other* field the bash CLI (or any other downstream
consumer) reads that the Rust struct didn't happen to model, and nothing
would catch it. The passthrough contract exists so that downstream text
processing of the raw JSON keeps working exactly as it does against the
Swift helper today.

## `sync`

Source: `apps/macos/Helper/SyncCommand.swift`. Scans installed browsers for
Chromium-based Claude session cookies, identifies each candidate session
against `GET /api/account`, and persists newly-found accounts. The storage
location and browser-cookie discovery mechanism are platform detail (see
`README.md`'s "Scope" section); only the following shape is shared:

- Identity comes from `GET /api/account` (line 33): the account's own `uuid`
  and `email_address`. A candidate whose `/api/account` call fails or returns
  no `uuid` (expired, revoked) is skipped with `  Skipping <profile> (session
  expired)`, not treated as an error for the whole run (line 33-36).
- A Claude account already present in the store is skipped with `  Skipping
  <profile> (already added)` (line 42-59) — skipped for *adding*; its stored
  plan tier is still refreshed, see the plan-refresh bullet below. "Already
  added" means **the same
  Claude account**, decided by `AccountIdentity.isDuplicate` — *not* the same
  browser profile, and never the same `orgId`. Two members of one
  organization are two accounts; two Claude accounts reached from one browser
  profile are also two accounts. `contract/cases/dedupe.json` is the rule.
- The `orgId` written to the account is chosen by
  `AccountIdentity.resolveOrgId` from the account's memberships, with the
  `lastActiveOrg` cookie as a preference only (line 61-65).
  `contract/cases/org-selection.json` is the rule. When it resolves to
  nothing the candidate is skipped with `  Skipping <profile> (no usable
  org)` — an unresolvable org is never persisted as a working account.
- Plan tier for a newly-added account comes from `OrgInfo.planHint` for the
  organization matching the resolved `orgId`, defaulting to `.pro` if no
  match is found (line 67-68) — this is a fallback distinct from
  `detectPlanTier`'s own `nil` case documented in `README.md`'s "Plan tier"
  section. `GET /api/organizations` is consulted for this and nothing else.
- A failed or empty `/api/organizations` fetch does not skip the candidate:
  session validity is established by `/api/account` above, not this call, so
  a failure here only leaves the plan at the `.pro` fallback and the account
  is still persisted (`apps/macos/Helper/SyncCommand.swift:67-68`,
  `apps/linux/helper/src/sync.rs:193-197`). The next bullet is what keeps that
  fallback from being permanent.
- The plan tier of an account skipped as a duplicate is **refreshed**
  (`apps/macos/Helper/SyncCommand.swift:48-57`,
  `apps/linux/helper/src/sync.rs:330-346` and `:359-366`): `sync` fetches
  `GET /api/organizations` with the candidate's freshly decrypted session key,
  takes the `planHint` of the org matching the **stored** account's `orgId`,
  and applies `refreshedPlan` (`README.md`'s "Refreshing a stored plan"). A
  write prints one extra line, immediately after the skip line:

      Updated plan: <profile> (<old wire value> -> <new wire value>)

  This is what makes the `.pro` fallback above self-healing: a tier that fell
  back because `/api/organizations` was down heals on the next `sync`, with no
  delete-and-re-add. The refresh never applies that fallback itself — an
  unresolvable hint leaves the stored tier alone. Nothing else about the
  stored account is written: not `sessionKey`, not `lastSynced`, not `status`.
  A stored account with no `orgId` has no org to match an entry against and is
  skipped without a refresh.
- Every persisted account carries `accountUuid` (line 82).
- **Linux only, no macOS counterpart:** a browser profile whose cookies are
  `v12` (xdg secret-portal, AES-256-GCM) needs a secret fetched per `app_id`,
  and when no candidate yields one that decrypts, the profile is skipped with
  `  Skipping <profile> (portal-encrypted cookies, no usable secret)`
  (`apps/linux/helper/src/sync.rs`). macOS cookies have no portal path, so
  this line can never appear there; a future port to another platform without
  the portal simply never emits it. The skip is a skip, not an error: the run
  continues and its exit code is unaffected. The Linux scan covers native and
  Flatpak installs of each browser (`~/.config/...` and
  `~/.var/app/<flathub id>/config/...`) plus Brave's official snap
  (`~/snap/brave/current/.config/...`); Chrome and Edge have no snap.
- The command always exits 0 once it finishes scanning (line 107), even when
  zero accounts were added; failure is only for the "no profiles with Claude
  sessions found at all" case (line 13-17, exit 1).
- The closing `Synced <N> account(s) successfully.` / `No new accounts to add
  (all already synced).` line counts **added** accounts only (line 101-105). A
  refreshed plan is not an add: a run that only healed a tier still reports
  "No new accounts to add".

## `add-key`

Source: `apps/macos/Helper/AddKeyCommand.swift` and
`apps/linux/helper/src/add_key.rs`. Adds or repairs one account from a session
key read on **stdin**. Never scans a browser.

The key is read from stdin and trimmed of surrounding whitespace and newlines.
It is never taken from `argv`, which is visible in `ps` and in shell history,
and never from the environment, which is readable at `/proc/<pid>/environ`.

Which of "add" and "repair" happens is decided by `AccountIdentity.isDuplicate`,
the same rule `sync` uses (`contract/cases/dedupe.json`) — not by a flag. What
the repair branch may write is `contract/cases/manual-key.json`: a stored
`orgId` is never rewritten, except from nothing.

| Situation | stderr | Exit |
|---|---|---|
| stdin holds no key | `No session key on stdin.` | 1 |
| `/api/account` rejects the key | `Session key not accepted (expired or invalid).` | 1 |
| Add, no chat org among the memberships | `No organization with chat access.` | 1 |
| Add, org resolved | `Added: <name> (<plan wire value>)` | 0 |
| Repair | `Updated key: <name>` | 0 |
| Repair, plan changed | plus `Updated plan: <name> (<old> -> <new>)` | 0 |
| Repair, no chat org among the memberships | plus `Warning: no organization with chat access; usage will not update.` | 0 |
| A failed write to the account store — **Linux only, no macOS counterpart** | `Could not write the account store.` | 1 |

The last row has no macOS branch: `HelperAccountStore.saveAccounts` returns
`Void` and reports no failure, while the Linux port's `store::save_accounts`
returns a `Result` its caller must not swallow. Do not add a matching branch
to the Swift command to make the two symmetrical.

`<name>` is the account's `email` when it has one, else the stored record's
`name` on the repair branch. On the add branch, an account whose `/api/account`
returns no `email_address` is named `Account <first 8 characters of its uuid>`.

The repair branch writes `sessionKey`, `status`, `lastSynced`, the `accountUuid`
and `email` backfills, and the plan through the same `refreshedPlan` rule
`sync` uses. It does not write `orgId` (except from nothing), `source`, the
profile fields, or `browser`: a key never changes which source a record has.

**The session key never appears in any of these lines.**

`sync`'s rules above are unchanged by this command.
