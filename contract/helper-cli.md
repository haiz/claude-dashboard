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

The subcommand sections below cite **symbols, not line numbers**. The Swift
helper has been restructured twice and every pinpoint citation in this document
went stale; a stale line number is worse than none. The two `main.swift` ranges
above are the exception, re-verified against the current file.

## `decrypt`

Source: `apps/macos/Helper/DecryptCommand.swift`.

Loads the persisted account list (`HelperAccountStore.loadAccounts()`). Output
is a **six-field projection**, not a serialized `Account`. The
`DecryptCommand.DecryptedAccount` shape:

| Field | Type | Source |
|---|---|---|
| `name` | string | `account.name` |
| `email` | string or null | `account.email` |
| `orgId` | string or null | `account.orgId` |
| `sessionKey` | string or null | see below |
| `plan` | string | `account.plan.rawValue` (a wire value from `account-schema.md`) |
| `status` | string | `account.status.rawValue` |

**Six fields means six keys, always.** A nil `email` or a nil `sessionKey` is
emitted as JSON `null`; the key is never dropped. A four- or five-key object
is a violation of this projection, not a shorthand for it. Swift needs an
explicit `encode(to:)` on `DecryptedAccount` to hold that line — the
synthesized `Encodable` calls `encodeIfPresent` for `Optional` stored
properties, which omits the key entirely — while `apps/linux/helper`'s
`BTreeMap` inserts every key unconditionally and needs nothing.

**Inclusion filter** (the `compactMap` guard in `DecryptCommand.run`): an
account is included only when `account.status == .active` **and**
`account.orgId != nil`. Note that `sessionKey` is *not* part of this filter —
an included account can still have `sessionKey: null` in the output if the
stored account itself has no session key.

**`sessionKey` value**: if the stored account has a `sessionKey`, the output
value is `CryptoService.decrypt(encrypted) ?? encrypted` — i.e. the plaintext
session key on successful decryption, but **silently falls back to the
still-encrypted ciphertext string** if decryption fails (wrong machine,
corrupted value, etc.). There is no error path for a failed decrypt; the
caller receives ciphertext masquerading as a plain value with no signal that
decryption failed.

**Encoding**: `JSONEncoder` with `[.prettyPrinted, .sortedKeys]` — output
keys are alphabetical (`email`, `name`, `orgId`, `plan`, `sessionKey`,
`status`), not struct-declaration order.

**Failure paths**, all three to stderr with exit code 1:
1. No accounts stored at all: stderr is exactly
   `No accounts found. Run: claude-dashboard-cli sync\n`.
2. Accounts stored but the inclusion filter above produces an empty list:
   stderr is exactly `No active accounts with session keys found.\n`. Note
   this message names "session keys" but the actual gate is
   `status == active && orgId != nil` (see above) — the wording is a
   misnomer in the source; a Rust port must match this stderr text
   verbatim, not the more accurate description of the filter.
3. JSON encoding itself fails (`guard let data = try?
   encoder.encode(decrypted), let json = String(data: data, encoding:
   .utf8) else { ... }`): stderr is exactly `Failed to encode accounts.\n`.
   This path exists for completeness of the contract even though it is not
   expected to trigger in practice — `DecryptedAccount` only holds `String`
   and `String?` fields, which `JSONEncoder` does not fail to encode.

Success: exit 0, pretty-printed JSON array printed to stdout.

## `usage <orgId> <sessionKey>`

Source: `apps/macos/Helper/UsageCommand.swift`. This is a **passthrough** —
it does not parse or reshape the upstream payload in any way.

Requires exactly two positional arguments; fewer prints
`Usage: claude-dashboard-helper usage <orgId> <sessionKey>\n` to stderr and
exits 1.

**`orgId` must be a single, unambiguous URL path segment.** It is rejected —
before any request leaves the process — when it is empty, or when it contains
any of `/`, `?`, `#`, a control character, or a whitespace character. This is
a rule about the *value*, not about what a URL parser happens to do with it:
both platforms spell it out character by character
(`apps/linux/core/src/api.rs`'s `validate_org_id`, and the matching guard in
`UsageCommand.run`) because Foundation's `URL(string:)` percent-encodes
whitespace and control characters into the path instead of refusing them, so
a parser-shaped check would let a malformed `orgId` through to a 404.

Request: `GET https://claude.ai/api/organizations/<orgId>/usage` with headers:
- `accept: */*`
- `content-type: application/json`
- `anthropic-client-platform: web_claude_ai`
- `Cookie: sessionKey=<sessionKey>`

These are the same four headers `UsageAPIService.makeRequest`
(`apps/macos/Shared/UsageAPIService.swift`) sends from the main app — the
helper does not add or omit anything.

Timeout: 15 seconds.

Exit 1, with the reason on stderr, on any of:
- An `orgId` the rule above rejects: `Invalid orgId.\n`.
- Timeout: `Request timed out.\n`.
- Network error: `Network error: <localizedDescription>\n`.
- Non-2xx HTTP status: `HTTP <status>\n`.
- A zero-length body, or a body that is not valid UTF-8: `Empty response.\n`.
  A `200` with a zero-length body takes this path too, on both platforms:
  success requires bytes, and a status alone is not a body. (This is easy to
  get wrong in Swift, where `String(data:encoding:)` returns `""` rather than
  `nil` for empty `Data`, so a decode-only check would accept it.)

Success: exit 0, the upstream response body printed to stdout **byte-for-byte
unchanged** — no trailing newline is appended on either platform.
`apps/macos/Helper/UsageCommand.swift` writes the upstream bytes straight to
`FileHandle.standardOutput` precisely because Swift's `print` would add one,
and `apps/linux/helper/src/usage.rs` uses `print!` rather than `println!` for
the same reason. Whatever JSON shape claude.ai returned that day survives,
including fields the Swift `UsageData` decoder does not know about.

### Why the passthrough shape matters

`cli/claude-dashboard-cli` (the bash wrapper) does not call the API
directly — it shells out to this helper and reads the printed body with `jq`
(`.five_hour.utilization`, `.seven_day.utilization`, each window's
`resets_at`, and the `limits[]` entry the Fable row is derived from), every
lookup guarded with a `// 0` or `// ""` default.

That guard is what once hid a dead lookup. The wrapper used to read
`.seven_day_sonnet.utilization // empty`; `seven_day_sonnet` is a field the
API no longer returns (see `README.md`'s "The Fable window" section) — the
third usage window has been `fable` for some time — and the default meant the
lookup produced an empty string rather than an error, so the CLI degraded
silently, rendering a blank Sonnet column instead of failing. It stayed that
way until `f373ecd` rewrote the row to come from `limits[]`.

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

- Identity comes from `GET /api/account` (`SyncCommand.runAsync`): the
  account's own `uuid` and `email_address`. A candidate whose `/api/account`
  call fails or returns no `uuid` (expired, revoked) is skipped with
  `  Skipping <profile> (session expired)`, not treated as an error for the
  whole run.
- A Claude account already present in the store is skipped with `  Skipping
  <profile> (already added)` — skipped for *adding*; its stored plan tier is
  still refreshed, see the plan-refresh bullet below. "Already added" means
  **the same Claude account**, decided by `AccountIdentity.isDuplicate` — *not*
  the same
  browser profile, and never the same `orgId`. Two members of one
  organization are two accounts; two Claude accounts reached from one browser
  profile are also two accounts. `contract/cases/dedupe.json` is the rule.
- The `orgId` written to the account is chosen by
  `AccountIdentity.resolveOrgId` from the account's memberships, with the
  `lastActiveOrg` cookie as a preference only.
  `contract/cases/org-selection.json` is the rule. When it resolves to
  nothing the candidate is skipped with `  Skipping <profile> (no usable
  org)` — an unresolvable org is never persisted as a working account.
- Plan tier for a newly-added account comes from `OrgInfo.planHint` for the
  organization matching the resolved `orgId`, defaulting to `.pro` if no
  match is found — this is a fallback distinct from `detectPlanTier`'s own
  `nil` case documented in `README.md`'s "Plan tier" section.
  `GET /api/organizations` is consulted for this and nothing else.
- A failed or empty `/api/organizations` fetch does not skip the candidate:
  session validity is established by `/api/account` above, not this call, so
  a failure here only leaves the plan at the `.pro` fallback and the account
  is still persisted (the `fetchOrganizations` call in
  `SyncCommand.runAsync`, and its counterpart in `run_sync` in
  `apps/linux/helper/src/sync.rs`). The next bullet is what keeps that
  fallback from being permanent.
- The plan tier of an account skipped as a duplicate is **refreshed**
  (`SyncCommand.runAsync`'s duplicate branch plus `refreshedStoredPlan`;
  `refresh_stored_plan`, `apply_refreshed_plan` and `refreshed_plan_for` in
  `apps/linux/helper/src/sync.rs`): `sync` fetches `GET /api/organizations`
  with the candidate's freshly decrypted session key,
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
- Every persisted account carries `accountUuid`.
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
- The command always exits 0 once it finishes scanning, even when zero
  accounts were added; failure is only for the "no profiles with Claude
  sessions found at all" case (exit 1).
- The closing `Synced <N> account(s) successfully.` / `No new accounts to add
  (all already synced).` line counts **added** accounts only. A
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

## Test coverage of the network layer

Every rule above is a rule about a *decision*, and every decision is covered by
an automated test on both platforms: `contract/cases/*.json` drives
`apps/linux/core/tests/contract_*.rs` and
`apps/macos/ClaudeDashboardTests/*ContractTests.swift`, and the per-platform
glue around those decisions has unit tests of its own (for example
`apply_refreshed_plan` in `apps/linux/helper/src/sync.rs`, whose tests also pin
the exact shape of the `Updated plan:` line by writing it through an
`impl Write` the test can read back, and
`SyncCommandTests.testHealWritesThePlanAndReportsItRightAfterTheSkipLine` in
`apps/macos/HelperTests/`, which asserts the same line *and its position*
directly after the skip line). The two implementations are still kept in step
by this document rather than by a shared test.

The HTTP calls themselves are covered on both platforms by tests that run the
**real helper binary** against a raw-TCP loopback server, which serves scripted
responses and records what arrived on the socket. That recording is the point:
a `URLSession` mock observes a `URLRequest`, never the bytes on the wire, so
the four request headers and the `Cookie: sessionKey=` value are now asserted
where they actually appear.

- `usage`: ten cases on macOS
  (`apps/macos/HelperTests/UsageCommandTests.swift`), nine on Linux
  (`apps/linux/helper/tests/usage_transport.rs`) — a 200 passed through
  byte-for-byte with the recorded request asserted, a zero-length body, a
  non-UTF-8 body, `401`, `500`, a connection closed mid-request, a silent
  server (the real 15-second timeout), missing arguments, and an `orgId`
  containing whitespace. macOS has a tenth, an `orgId` containing `/`; Linux
  pins that character in `validate_org_id`'s own unit tests
  (`apps/linux/core/src/api.rs`) rather than through the process.
- `add-key`: six cases per platform — empty stdin, a rejected key, an add with
  no chat org, a successful add, a repair, and a repair that changes the plan.
  Every case pins stderr exactly. The successful add and the key-rewriting
  repair go further, on both platforms alike: each asserts explicitly that the
  plaintext never appears on stderr, and checks the written record field by
  field — including that the stored key is neither the plaintext nor the value
  it replaced, and that it decrypts back to what went in on stdin. The
  plan-changing repair asserts the written tier.
  (`apps/macos/HelperTests/AddKeyCommandTests.swift`,
  `apps/linux/helper/tests/add_key_transport.rs`)
- `decrypt`: touches no network, but is driven as a real process all the same —
  six cases on macOS (`apps/macos/HelperTests/DecryptCommandTests.swift`),
  five on Linux (`apps/linux/helper/tests/decrypt_golden.rs`). Both cover the
  projection, the inclusion filter, the alphabetical key order, the
  null-rather-than-omitted rule for an absent value, and the ciphertext
  fallback; macOS adds a round trip proving the at-rest key derivation agrees
  across a process boundary.
- `sync` is deliberately not driven this way: neither platform has a seam for
  the browser-cookie scan at the process level, so a real-binary run would read
  real cookie databases. Its decisions are covered in-process instead — nine
  tests in `apps/macos/HelperTests/SyncCommandTests.swift` drive
  `SyncCommand.runAsync(env:)` with an injected `Environment` and
  `MockURLProtocol`, so every stderr line and exit code this document specifies
  for `sync` is asserted without touching a browser, the Keychain or the real
  account store. On Linux the same decisions are covered by
  `apps/linux/helper/src/sync.rs`'s own unit tests plus
  `apps/linux/helper/tests/sync_dedupe.rs`, which pins the dedupe key against
  `contract/cases/dedupe.json`.

Two environment variables make the process-level tests possible. Both are
**platform detail, not contract**, and neither changes observable behaviour
when unset:

- `CLAUDE_DASHBOARD_API_BASE` (both platforms) redirects requests at a loopback
  server. Only a plain-`http` origin on `127.0.0.1` or `localhost` is honoured;
  any other value is ignored **silently** and the client uses
  `https://claude.ai`. Silently, because this document specifies stderr byte
  for byte, so a warning would itself be a contract violation.
- `CLAUDE_DASHBOARD_DEFAULTS_SUITE` (macOS only) overrides the UserDefaults
  suite the account store lives in. `cfprefsd` does not follow `HOME`, so this
  is the counterpart of `XDG_CONFIG_HOME`, which already redirected the Linux
  store (`apps/linux/core/src/store.rs`'s `accounts_path`).

Both are read inside the production code paths, which makes them different in
kind from `SyncCommand.Environment` — an in-process seam with no default value,
so that a caller who forgets one cannot scan real cookie databases. That design
is unchanged. One consequence follows from the first variable: `sync` composes
its requests from the same base URL, so it too would follow a loopback
override, even though no test drives it that way.

The macOS **app** is covered the layer above: `DashboardViewModel` is driven
through `MockURLProtocol`, so the GUI's own plan refresh is tested end to end,
including a failing `GET /api/organizations`
(`ClaudeDashboardTests/DashboardViewModelTests.swift`). Between the app bundle
and the helper bundle, every macOS caller of `refreshedPlan` has a test.

What the loopback tests do **not** reach, and what therefore still rests on a
manual run:

- **TLS.** The loopback server is plain HTTP; neither client's TLS stack is
  exercised.
- **The real claude.ai endpoint.** No automated test talks to it.
- **`sync`'s browser scan**, per the bullet above.

Multiple `Set-Cookie` headers are a fourth gap of a different kind: no
subcommand observably consumes the refreshed key (`usage` ignores it by design,
the other calls discard cookies), so it cannot be asserted through the process
shell at all. `parse_session_key` has its own unit tests, and the app's
in-process tests cover the GUI's use of it.

The manual record below is refreshed **per release**, alongside the two suites
`scripts/release.sh` already runs. Last verified
2026-09-04 on macOS, with `claude-dashboard-helper` built from the working
tree: a first `sync` printed `Skipping <profile> (already added)` for each
stored account with no `Updated plan:` line (every tier already correct); one
stored tier was then set to a deliberately wrong value, and the next `sync`
printed exactly one `Updated plan: <profile> (Pro -> Max)` line immediately
after that profile's skip line, still closing with `No new accounts to add
(all already synced)`. A field-by-field diff of the account store across the
healing run showed the plan as the only value written — `sessionKey`
(AES-GCM, so a rewrite would change the ciphertext), `lastSynced` and `status`
were byte-identical.
