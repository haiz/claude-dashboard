# contract/

This directory is the source of truth for behaviour that must be identical
between the macOS (Swift) app and the future Linux (Rust) app. If a rule
lives here, both implementations must satisfy it. If a rule is *not* here,
each platform is free to do it its own way.

The executable form of this contract is the JSON case files under
`contract/cases/`. Every case is self-contained — its input is inline in
the case object (`input`, `org`, `projected_seconds`, depending on the
file), so there is **no `contract/fixtures/` directory and none is
coming**. A real captured usage payload was used while writing these
documents and was deliberately withheld from the repository: it carries the
account's real `spend` figure and roughly eight top-level keys that are
unreleased internal codenames, and this repository is public. Nothing in
this contract depends on that payload; if you want a live response beyond
what the cases carry, capture your own. This README, and the sibling `.md`
files in this directory, are the prose that explains *why* those cases say
what they say — every claim below is checked against the current Swift
source, with a citation, so the Rust implementation is built against what
the code actually does rather than against a plausible-sounding description
of it.

## Scope

**Shared (must match):**
- Plan-tier detection logic (`account-schema.md`, this file's "Plan tier"
  section).
- The `UsageData` decode shape, including how the Fable window is derived
  (this file's "The Fable window" section).
- Burn-rate levels and thresholds, **and the stateful history rules that
  produce the projected time those thresholds are applied to**, and the
  resulting sort order (this file's "Burn-rate levels", "Burn-rate
  projection", and "Sort order" sections). The thresholds alone are not
  enough: two implementations fed the same poll history but projecting
  differently will display different animals.
- **Session-key refresh, and the mapping of HTTP 401/403 to the `expired`
  account status** (this file's "Session-key refresh and auth expiry"
  section). This is shared because the resulting account status is
  user-visible, not because the transport is; *how* the HTTP request is
  issued remains platform detail.
- The helper CLI's three subcommands — `decrypt`, `usage`, `sync` — as
  externally observable behaviour (input/output shape, exit codes, stderr
  text): `helper-cli.md`.
- The `Account` JSON schema and its two backward-compatibility defaults:
  `account-schema.md`.
- The `usage_logs` SQLite schema and its compression semantics:
  `usage-log.md`.
- **Account identity and org selection** — which account a session belongs to,
  and which org its usage is polled from (this file's "Account identity" and
  "Org selection" sections, `cases/dedupe.json`, `cases/org-selection.json`).

**Platform detail (deliberately free to differ):**
- **Cookie decryption.** How a session key is extracted from the browser's
  cookie store is macOS/Chromium-specific (`apps/macos/Shared/BrowserCookieService.swift`
  uses PBKDF2-SHA1 + AES-128-CBC keyed off a Safe Storage password read
  from the Keychain **per browser**, under that browser's own service name —
  `"Chrome Safe Storage"`, `"Arc Safe Storage"`, `"Brave Safe Storage"`,
  `"Microsoft Edge Safe Storage"` (`apps/macos/Shared/Browser.swift:35-42`,
  read at `BrowserCookieService.swift:255` / `:261`). A Linux implementation
  reading a different browser's cookie store needs none of this.
- **At-rest session-key encryption.** `apps/macos/Shared/CryptoService.swift`
  derives an AES-GCM key via HKDF-SHA256 seeded from the machine's
  `IOPlatformUUID` (read in `hardwareUUID()`, lines 39–53). This ties
  ciphertext to one specific Mac; a Linux port must pick its own at-rest
  scheme, and the two are not expected to interoperate (see
  `account-schema.md`'s note on `sessionKey`).
- **Storage backend.** The macOS app persists accounts as a JSON blob in
  `UserDefaults` (`apps/macos/ClaudeDashboard/Services/AccountStore.swift:44-55`,
  `apps/macos/Helper/HelperAccountStore.swift:8-23`). The *shape* of that
  JSON is contract (`account-schema.md`); where and how it is stored is
  not.
- **UI.** Menu bar layout, window chrome, colors, charts — all platform
  detail. (The two circles / segmented countdown / animal emoji are
  intentional macOS UI design choices, not contract material.)

## The case-file convention

Each file under `contract/cases/` is a JSON array of
objects. Every object has a `name` field identifying the case; every other
field is specific to that case file and is documented in the matching
section below (or in the `.md` file for that surface). Both test suites —
the Swift `XCTest` suite and the Rust test suite — iterate the array
in order and report the failing case's `name` on failure, so a case name
should be descriptive enough to locate the assertion it's checking without
opening the JSON.

## Timestamps

Every expected instant in a `contract/cases/*.json` file is an **integer
Unix second, truncated toward zero** — not rounded, not fractional.

Rationale: `UsageData.decode(from:)` (`apps/macos/Shared/UsageData.swift:30-54`)
tries `ISO8601DateFormatter` with `.withFractionalSeconds` first (lines
36-40), falling back to a formatter without fractional seconds only if that
fails (lines 42-46). Real API responses do carry fractional seconds — a
decode test's fixture has `"resets_at": "2026-07-03T17:00:00.283013+00:00"`
(`apps/macos/ClaudeDashboardTests/UsageDataTests.swift:84`) — so on the
Swift side the decoded `Date` genuinely carries sub-second precision. A
Rust `i64` epoch field cannot represent that fraction. Truncating to whole
seconds is the one representation both an `NSDate`/`Date` value and an `i64`
can produce identically, so every expected timestamp in a case file must be
computed by truncating (not rounding) to the second.

**This rule governs expected values inside `contract/cases/*.json` and
nothing else.** In particular it is *not* the account store's wire format:
`Account.lastSynced` is serialised by a bare `JSONEncoder()` as a
*fractional Double on the 2001-01-01 epoch*, not an integer Unix second.
See `account-schema.md`'s "Wire encoding of the non-string scalars" before
modelling that field.

## Plan tier

Plan tier is derived **only** from `GET /api/organizations` (specifically
`UsageAPIService.fetchOrganizations`, `apps/macos/Shared/UsageAPIService.swift:49-73`)
— **never** from the usage endpoint. The top-level `CLAUDE.md` *previously*
said plan tier was detected "from `extra_usage` response field". That claim
was false and **has since been corrected on this same branch**: the
`UsageAPIService` entry under `CLAUDE.md`'s "Services Layer" now states that
plan tier comes from the organizations endpoint's `capabilities` and never
from the usage response. This section is where the reasoning behind that
correction lives — and it remains the authoritative record for anyone
reading a fork or an older checkout that still carries the `extra_usage`
wording.

The code's own comment states why (`apps/macos/Shared/UsageAPIService.swift:77-80`):

> The usage endpoint carries no reliable plan-tier signal — `extra_usage` is
> a pay-as-you-go overage toggle (it flips to is_enabled=false when out of
> credits) and has no tier/multiplier field. Plan tier comes solely from the
> organizations endpoint's capabilities (see `detectPlanTier`).

A real `extra_usage` payload, from a test fixture
(`apps/macos/ClaudeDashboardTests/UsageAPIServiceTests.swift:104`):
`{ "is_enabled": false, "disabled_reason": "out_of_credits" }` — confirming
it is a credit toggle, not a plan marker.

The actual derivation, `detectPlanTier` (`apps/macos/Shared/UsageAPIService.swift:101-123`),
checked in this order against one organization's dict + `capabilities` array:

1. If the org's raw JSON (serialized back to a lowercased string) contains
   `"max_20x"` or `"max20x"` → `.max20x` ("Max 20x").
2. Else if it contains `"max_5x"` or `"max5x"` → `.max5x` ("Max 5x").
3. Else if `capabilities` contains `"claude_pro"` → `.pro` ("Pro"). Checked
   before the chat fallback because Pro orgs also carry `"chat"`.
4. Else if `capabilities` contains `"claude_max"` → `.max200` (wire value
   `"Max"` — see the naming trap in `account-schema.md`).
5. Else if `capabilities` contains `"chat"` → `.max200` (wire value `"Max"`)
   — a consumer chat org without the Pro marker, tier unknown.
6. Else → `nil` (not a plan the dashboard displays, e.g. an API-only org).

Per the code's own comment (lines 97-100 immediately above `detectPlanTier`,
"the 5x vs 20x distinction is not exposed anywhere"), steps 1-2 are markers
the implementation scans for defensively; this document cannot independently
verify whether any live API response ever sets them, only that the code
comment asserts it does not currently happen. Treat that line as sourced
from the implementation's own comment, not as an independently verified
fact. Practically, this means today every consumer Max account — 5x or 20x
— resolves to the generic `"Max"` tier via step 5, not step 1/2.

### Refreshing a stored plan

The tier above is derived only when something writes it. Two things make a
*stored* tier wrong: `GET /api/organizations` failing at the moment the
account was added (`helper-cli.md`'s "sync" section defaults that case to
`.pro`), and a real plan change on the account afterwards. One rule corrects
both — `refreshedPlan` (`apps/macos/Shared/UsageAPIService.swift`, mirrored by
`plan::refreshed_plan` in `apps/linux/core/src/plan.rs`) — evaluated against
the stored plan and a freshly fetched hint:

1. The hint is `nil` — the fetch failed, the response has no org matching the
   account's `orgId`, or `detectPlanTier` returned `nil` → **no write**. A
   network blip must never overwrite a known-good tier, and the add-time
   `.pro` default is never re-fabricated on a refresh.
2. The hint equals the stored plan → **no write**.
3. Otherwise → **write the hint**. An upgrade and a downgrade are the same
   case: the freshly fetched org is authoritative.

`contract/cases/plan-refresh.json` is the rule.

Every writer applies it: the macOS app's `DashboardViewModel.refreshAll` on
each refresh cycle, and both helpers' `sync` for a candidate that dedupe
resolved to an account already in the store (see `helper-cli.md`'s "sync"
section). The consequence worth stating plainly: **a wrong tier heals on the
next refresh or the next `sync`**, with no delete-and-re-add. Before this rule
was shared, only the macOS GUI healed — a CLI-only user on either platform
kept a wrong badge until they deleted the account and synced again.

## The Fable window

`fable` is not a top-level field of the usage response. It does not exist
as `seven_day_fable` or any similar key. It is derived from the `limits`
array: the first entry whose `scope.model.display_name == "Fable"`, reading
that entry's `percent` field (not `utilization` — the top-level `five_hour`/
`seven_day` objects use `utilization`, but entries inside `limits[]` use
`percent`; these are two different field names for the same kind of value).

Source: `apps/macos/Shared/UsageData.swift:90-103` (the custom
`init(from:)`). Specifically:
- Line 99: `.first { $0.scope?.model?.displayName == "Fable" }`
- Line 100: `.map { UsageLimit(utilization: $0.percent ?? 0, resetsAt: $0.resetsAt) }`

The bash CLI derives the same window in `cli/claude-dashboard-cli`
(`fable_from_usage_json`) for its third usage row, checked against
`cases/usage-decoding.json` by `scripts/test-cli-usage.sh`.

If a `weekly_scoped` / `Fable` entry exists but its own `percent` is
missing, utilization defaults to `0` (it is *not* treated as "no Fable
window" — that only happens when no matching entry exists at all, or the
`limits` array itself is absent, in which case `fable` decodes to `nil` and
the UI hides the gauge).

**A malformed `limits` is silently swallowed, not surfaced.** Line 97 is
`(try? container.decode([LimitEntry].self, forKey: .limits)) ?? []` — a
`try?`, which discards failure exactly as it discards absence. So if
`limits` is present but is not an array, or contains an element that is not
an object, or an entry whose `percent` is not a number, or **any** entry
(not only the Fable one) whose `resets_at` is present but unparseable by the
two ISO8601 formatters, the array decode throws, the throw is dropped, the
array becomes `[]`, and `fable` ends up `nil` with **no error reported
anywhere**. A Rust port must reproduce that silence: a bad `limits` payload
is not a failed decode of the response, it is simply "no Fable window".

**`five_hour` and `seven_day` are REQUIRED — do not model them as
optional.** `UsageData.swift:92-93` uses `container.decode(...)`, not
`decodeIfPresent`, so a response missing either key, or carrying either with
a missing or non-numeric `utilization`, or with a `resets_at` that is
present but unparseable, throws and fails the entire
`UsageData.decode(from:)`. Note the asymmetry with the paragraph above: an
unparseable `resets_at` inside `limits[]` is swallowed into `fable = nil`,
while an unparseable `resets_at` on `five_hour`/`seven_day` fails the whole
decode. Absent or JSON-`null` is a different case from unparseable —
`UsageLimit.resetsAt` is `Date?` (`UsageData.swift:5`) and `null` is
preserved as `nil`, pinned by `contract/cases/usage-decoding.json`'s case
"null resets_at is preserved, not defaulted".

`seven_day_sonnet` is a field the current API no longer returns. The
decoder's `RootKeys` (`apps/macos/Shared/UsageData.swift:60-64`) declares no
case for it, so if a `seven_day_sonnet` key were ever present in a response
it would be dropped the same way `JSONDecoder` drops any unrecognized key
under a keyed container — there is no explicit skip/guard for it in the
code, it simply isn't looked for. The top-level `CLAUDE.md` *previously*
described `UsageData` as carrying "5-hour, 7-day, and Sonnet windows". That
was false — the third window is Fable, not Sonnet, and has been since the
`fable` field replaced it — and it **has since been corrected on this same
branch**: the `UsageData` entry under `CLAUDE.md`'s "Models" heading now
describes the 5-hour and 7-day windows plus an optional Fable window derived
from `limits`, and calls out `seven_day_sonnet` as a removed field the
decoder ignores. This section is where the reasoning behind that correction
lives.

A third place carried the same stale claim, found while writing this
document: the top-level `README.md`'s "What each row means" section listed a
bar labelled **S** as "7-day Sonnet-specific utilization (Max plans only)".
That wording is gone. The surviving statement in that same section reads:

> The CLI can also print a third bar labelled **S**, but it reads a
> `seven_day_sonnet` field the Claude.ai API no longer returns, so that row
> never appears today.

The row is still coded into the bash CLI (`cli/claude-dashboard-cli`); see
`helper-cli.md` for why it never renders in practice. (Quoted rather than
cited by line number on purpose — this document has already been wrong once
about a line number in a file it does not own.)

## Burn-rate levels

Source: `apps/macos/ClaudeDashboard/Models/UsageLogModels.swift:44-61`
(`BurnRateResult.animals` and `BurnRateResult.fromProjectedTime`).

Given a projected time-to-100%, in hours:

| Condition (hours) | Level | Animal |
|---|---|---|
| `> 5`     | 1 | 🐌 |
| `> 3`     | 2 | 🐢 |
| `> 1.5`   | 3 | 🐇 |
| `> 0.5`   | 4 | 🐎 |
| otherwise | 5 | 🐆 |

Every comparison is **strictly greater-than** (`if hours > 5 { level = 1 }
else if hours > 3 { level = 2 } ...`). A projection of *exactly* 5.0 hours
fails `> 5` and falls through to `> 3` (true), so it is **level 2, not
level 1**. The same strict-inequality rule applies at every other
boundary (exactly 3.0h → level 3, exactly 1.5h → level 4, exactly 0.5h →
level 5).

## Burn-rate projection

`fromProjectedTime` above is only the *last* step. The number handed to it —
projected seconds to 100% — is produced by a stateful, in-memory tracker
whose rules a port cannot guess from the thresholds alone. Source:
`apps/macos/ClaudeDashboard/Services/BurnRateTracker.swift` (107 lines;
`record(...)` spans lines 25-106).

### The call

`record(accountId:window:utilization:resetsAt:recordedAt:)` is called once
per window per refresh and returns `BurnRateResult?`. A `nil` return means
"no animal to display yet" — it is not an error. `recordedAt` defaults to
`Date()` (line 30) but is injectable, which is how the Swift tests drive it.

### History key and lifetime

- History is a dictionary keyed by
  `"\(accountId.uuidString)_\(window.rawValue)"` (line 32) — one independent
  history per *(account, window)* pair. `uuidString` is the uppercase
  hyphenated form; `window.rawValue` is the `UsageWindow` **integer**:
  `fiveHour = 0`, `sevenDay = 1`, `fable = 3` (`UsageLogModels.swift:4-9` —
  `2` is deliberately skipped, it belonged to the retired Sonnet window).
- The history lives only in memory, inside an `actor` (lines 4, 19). It is
  never persisted. A process restart wipes it, so the first poll after every
  launch takes rule 1 below.
- Each entry holds three things (lines 13-17): `prev` and `current`, each a
  `Measurement` of utilization + `recordedAt` + `resetsAt` (lines 7-11), and
  `lastRate`, a rate in **percent per second**.

### Unconditional side effect

Before any history logic runs, every call writes one row to the usage log
store (lines 36-39) with `isLimited = utilization >= 100.0` (line 33). This
happens on the `nil`-returning paths too. `isLimited` is a log field only;
it plays no part in the projection.

### The rules, in the order the code evaluates them

1. **First measurement** (lines 45-49). No entry for the key, or the entry's
   `current` is `nil`: store
   `HistoryEntry(prev: nil, current: <new>, lastRate: nil)`, return `nil`.
2. **Different reset cycle** (lines 52-55). `resetsAt != current.resetsAt` —
   **exact** instant equality, sub-second precision included — discards the
   history: store a fresh `HistoryEntry(prev: nil, current: <new>,
   lastRate: nil)`, return `nil`.
3. **Utilization decreased** (lines 58-61). `utilization < current.utilization`:
   the same full reset as rule 2, return `nil`. (Read as an anomaly or an
   unnoticed post-reset.)
4. **Utilization increased** (lines 64-79). `utilization > current.utilization`:
   - `deltaPercent = utilization - current.utilization`
   - `deltaTime = recordedAt - current.recordedAt`, in seconds
   - If `deltaTime <= 0`: return `nil` **without touching the history**
     (line 67). The new measurement is discarded and `current` stays the
     older one — it was still logged, per "Unconditional side effect".
   - `rate = deltaPercent / deltaTime`, percent per second (necessarily
     `> 0` on this path)
   - `remaining = 100.0 - utilization`
   - `projectedTime = remaining / rate`, in seconds
   - Store `prev = <old current>`, `current = <new>`, `lastRate = rate`;
     return `fromProjectedTime(projectedTime)`.
   - There is **no** `remaining > 0` guard here. At `utilization == 100` the
     projection is `0` → level 5; above `100` it is negative, every `>`
     comparison inside `fromProjectedTime` fails, and it is level 5 again.
5. **Utilization unchanged** — exact `Double` equality, i.e. neither rule 3
   nor rule 4 fired (lines 82-105). Let `gap = recordedAt - current.recordedAt`,
   in seconds:
   - **Stale, `gap >= 300`** (lines 84-89; 300 s = 5 minutes, and *exactly*
     300 counts as stale): set `current = <new>` and **drop the carried
     rate** (`lastRate = nil`), leaving `prev` as it was; return `nil`.
     After five flat minutes the last observed rate is no longer trusted.
   - **Fresh, `gap < 300`, no carried rate** (lines 92-96): if `lastRate` is
     `nil` or `prev` is `nil`, set `current = <new>`, leave `lastRate`
     untouched, return `nil`. Two notes for a reimplementation: `lastRate`
     non-`nil` already implies `prev` non-`nil` (both are only ever set
     together at lines 73-75, and only `lastRate` is cleared, at line 86), so
     the `prev` half of that guard is redundant — a port that models the two
     fields independently must not let them diverge. And a **negative** gap,
     from an out-of-order or clock-skewed poll, falls into this `< 300`
     branch, not the stale one.
   - **Fresh, `gap < 300`, carried rate available** (lines 98-105):
     - `remaining = 100.0 - utilization`
     - If `remaining <= 0`: return `fromProjectedTime(0)` — level 5 —
       **without updating the history** (lines 99-101); `current` stays the
       older measurement.
     - Otherwise `projectedTime = remaining / lastRate`; set
       `current = <new>` (carrying `lastRate` and `prev` forward unchanged)
       and return `fromProjectedTime(projectedTime)`.

### Consequence of the macOS caller's `resetsAt` substitution

This is caller behaviour rather than tracker behaviour, but it decides what
a user actually sees, so a port copying the caller must copy it knowingly.
`DashboardViewModel` (lines 231-249) substitutes a freshly computed
`Date().addingTimeInterval(...)` whenever a window's `resets_at` is `nil` —
`18000` for the 5-hour window (line 236), `604800` for 7-day (line 241) and
for Fable (line 247). That substitute is a *new instant on every refresh*,
so rule 2's exact-equality check fails on every poll: **a window whose
`resets_at` is `null` resets its history every poll and therefore never
produces a burn rate.** Only windows with a real `resets_at` ever show an
animal. The Fable window is polled at all only when `fable != nil`
(line 243).

## Sort order

Source: `DashboardViewModel.sortStates()`
(`apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift:426-442`)
and `DashboardViewModel.burnRate(for:)` (lines 366-382).

This is a **three-tier** ordering, not a flat sort by burn rate:

1. **Pinned first.** Any account with `isPinned == true` sorts above any
   account with `isPinned == false`, unconditionally (line 430-432).
2. **Active Claude Code account next, but only if no account anywhere is
   pinned.** `anyPinned` is computed once per sort (line 427) from the
   *entire* list, not the pair being compared. If it's `false`, the account
   matching `activeClaudeCodeEmail` (`isActiveClaudeCodeAccount`, lines
   309-312, matched by `account.email`) sorts above accounts that don't
   match (lines 434-438). If any account anywhere is pinned, this tier is
   skipped entirely — an unpinned "active Claude Code" account does **not**
   outrank other unpinned accounts once something is pinned.
3. **Burn rate, descending**, as the final tiebreaker (line 440):
   `burnRate(for:) = utilization / timeRemaining`, using the **5-hour**
   window's `utilization` (line 373) — not 7-day, not Fable. `timeRemaining`
   is `max(resetsAt.timeIntervalSinceNow, 60)` when `resetsAt` is present
   (a 60-second floor prevents division blow-up near a reset), or a
   hardcoded `18000` seconds (5 hours) when `resetsAt` is `nil` (lines
   374-379). An account that is not `.active`, or has no usage data yet,
   gets a sentinel burn rate of `-1` (lines 368-371) — guaranteed to sort
   below every account with a real rate (since a real rate is always
   `>= 0`), i.e. expired/error/no-data accounts sink to the bottom within
   whatever tier they're in.

## Session-key refresh and auth expiry

Classified as **Shared** in the Scope section above, because the account
status it produces is user-visible. Source:
`apps/macos/Shared/UsageAPIService.swift`.

`validateResponse` (lines 135-149) is applied to every response, from both
the usage and the organizations endpoint:

1. Not an HTTP response → `UsageAPIError.invalidResponse` (lines 136-138).
2. Status is exactly `401` **or** `403` → `UsageAPIError.authExpired`
   (lines 140-142). No other status reaches this case.
3. Status outside `200...299` → `UsageAPIError.httpError(statusCode:)`
   (lines 144-146).
4. Otherwise the response is returned unchanged.

`parseSessionKey` (lines 151-165) runs on the successful responses of
`fetchUsage` (line 42) and `fetchFullUsage` (line 91) — but **not** on
`fetchOrganizations`, which discards its validated response (line 56):

1. Read the `Set-Cookie` response header. Absent → `nil` (lines 152-154).
2. Split that header value on `;`, trim whitespace from each component, and
   return the first component beginning with the literal prefix
   `sessionKey=`, with the prefix removed (lines 156-162). The value is
   taken verbatim — not URL-decoded, not unquoted, no length or format
   check.
3. No such component → `nil` (line 164).

Note that step 1 reads *one* header value and step 2 splits only on `;`;
the code has no handling for a server sending several separate `Set-Cookie`
headers. A port must make sure it still finds the `sessionKey` cookie in
that case, since HTTP clients differ in how they surface repeated headers.

The user-visible consequences, which are the reason this is contract:

- A non-`nil` parse result is persisted as the account's new session key
  (`DashboardViewModel.swift:175-177`), replacing the one just sent.
  Requests carry it as the raw header `Cookie: sessionKey=<value>`
  (`UsageAPIService.swift:131`).
- `authExpired` becomes the account status `expired`, written back to the
  store (`DashboardViewModel.swift:184-185` and `216-219`), and an
  `expired` account is skipped by subsequent refreshes
  (`DashboardViewModel.swift:163`). Every other error leaves `status`
  untouched and surfaces only as a transient per-card message
  (lines 186-189). A Linux implementation that also mapped, say, `429` or a
  network failure onto `expired` would silently retire accounts that the
  macOS app keeps refreshing.

## Account identity

An account is identified by its own uuid, from `GET /api/account`'s `uuid`
field (`fetchAccount`, `apps/macos/Shared/UsageAPIService.swift:86-113`),
stored as `accountUuid` (`apps/macos/Shared/Account.swift:27`,
`apps/linux/core/src/model.rs:53`). Dedupe compares that and nothing else, except
for one legacy fallback: a stored record written before `accountUuid` existed
has none, and matches on `email` compared after Unicode lowercasing: both
strings are lowercased (`lowercased()` in Swift, `to_lowercase()` in Rust) and
then compared with each language's string equality. Those equalities are not the
same primitive either — Swift's `==` is canonical-equivalence-aware, Rust's
compares bytes — which is exactly the normalization limit below.

Both sides must run that same operation, not merely agree on today's inputs. An
earlier revision paired Swift's `caseInsensitiveCompare` with Rust's
`eq_ignore_ascii_case`: one folds Unicode, the other only ASCII, and every case
in `cases/dedupe.json` was ASCII, so both suites stayed green while the two
platforms answered differently for the first accented address. The accented case
in that file pins the fold.

Normalization is **not** part of the rule and is a known limit. Swift's `==`
treats NFC and NFD as equal where Rust's compares bytes, so the same accented
address written in two normal forms still resolves differently on the two
platforms. Every address the rule sees arrives from one place, `/api/account`'s
`email_address`, in one form; the case file keeps both of its strings in NFC.

`orgId` is **not** an identity. Every member of a company organisation shares
its uuid, so keying dedupe on it rejects the second and every later colleague.
The rule is implemented at `apps/macos/Shared/AccountIdentity.swift:74-97`
(`duplicateIndex`/`isDuplicate`) and `apps/linux/core/src/identity.rs:64-84`
(`duplicate_index`/`is_duplicate`), and driven by `cases/dedupe.json`.

E-mail comes from `/api/account`'s `email_address`. It is never recovered by
parsing an organisation's name: observed live data includes orgs named
`<Name>‘s Individual Org` with U+2018, which no `"'s Organization"` pattern
matches.

## Org selection

`orgId` is the org whose `/usage` an account is polled against, and nothing
else. It is resolved as:

1. the cookie's `lastActiveOrg`, if that uuid is a chat org in the account's
   memberships
2. otherwise the first chat org, in the order `/api/account` returned them
3. otherwise none — the account is not configurable and must be reported to the
   user rather than persisted

A "chat org" is one whose `capabilities` contain `"chat"`, compared with the
same Unicode lowercasing the dedupe rule above uses, for the same reason
(`isChatOrg`, `apps/macos/Shared/AccountIdentity.swift:10-12`; `is_chat_org`,
`apps/linux/core/src/identity.rs:17-19`). The gate excludes
API-console orgs, whose capabilities are `["api", "api_individual"]` and whose
`/usage` is not meaningful.

The organisation's *name* is never inspected. An earlier revision preferred the
org whose name ended in `"'s Organization"`; on an account that belongs to both
a company org and its own personal org, that selects the personal org, whose
`/usage` returns every window as `null`. A personal org remains eligible on its
own merits — for a personal Pro or Max account it is the correct answer.

Implemented at `apps/macos/Shared/AccountIdentity.swift:49-56` (`resolveOrgId`) and
`apps/linux/core/src/identity.rs:29-41` (`resolve_org_id`), driven by
`cases/org-selection.json`.

`resyncCore` obeys the same rule. It re-reads one stored account's browser
cookies and resolves `orgId` through `resolveOrgId` against the memberships
`/api/account` returns for that session, never from the cookie alone —
`apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift:346-362`. It is
the single writer behind both re-sync entry points: `resyncAccount`, used by each
account card's individual resync action
(`apps/macos/ClaudeDashboard/Views/DashboardWindow.swift:111`,
`apps/macos/ClaudeDashboard/Views/MenuBarPopover.swift:108`), and `resyncAll`,
used by the "Re-sync All" button
(`apps/macos/ClaudeDashboard/Views/SettingsView.swift:89`).

The refresh that follows a re-sync is scoped to the accounts that re-synced
successfully (`refreshAll(only:)`, lines 380 and 393). An unscoped pass would
overwrite the message left on a card whose re-sync failed, and re-syncing N
accounts would cost N whole-fleet refresh passes instead of one.

Resync updates a record that already holds a working `orgId`, so three outcomes
differ from the add-an-account path:

- `/api/account` unreachable. The new session key is saved and the account is
  marked `active`, while `orgId`, `accountUuid` and `email` keep their stored
  values (lines 343-344, 353). `refreshAll` skips `expired` accounts, so marking it
  active is what makes any later retry possible; leaving `orgId` alone is what
  keeps an unreadable session from erasing a correct one.
- No chat org among the memberships. The stored `orgId` is kept and the card
  reports it (lines 365-371). Rule 3's "never persisted" governs adding an
  account; an existing one is reported rather than repointed or blanked.
- The session identifies a different account than the stored `accountUuid`.
  Nothing is written and the card reports it (lines 336-340) — a profile signed
  in to another Claude login must not have its session key copied onto this
  record, which is the same collision `isDuplicate` exists to prevent. A legacy
  record without `accountUuid` has nothing to compare and is backfilled instead.

A **pasted session key** obeys a narrower rule than either. It carries no
`lastActiveOrg`, so on the add path rule 2 applies: the first chat org. On the
repair path the stored `orgId` is **not** rewritten at all — re-resolving would
run rule 2 and demote an `orgId` that rule 1 had resolved correctly from the
cookie, on an account belonging to more than one chat org. The single exception
is a stored `orgId` of `nil`: there is nothing to demote, and it is the same
backfill semantics `accountUuid` has. Implemented at
`apps/macos/Shared/ManualKey.swift` and `apps/linux/core/src/manual_key.rs`,
driven by `cases/manual-key.json`.

The Linux side has no resync. `sync` and `add-key` are its two writers, and
`add-key` obeys the pasted-key rule above rather than the resync one.
