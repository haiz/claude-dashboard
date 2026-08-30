# contract/

This directory is the source of truth for behaviour that must be identical
between the macOS (Swift) app and the future Linux (Rust) app. If a rule
lives here, both implementations must satisfy it. If a rule is *not* here,
each platform is free to do it its own way.

The executable form of this contract is the JSON case files under
`contract/cases/` (added by later tasks), backed by fixture payloads under
`contract/fixtures/`. This README, and the sibling `.md` files in this
directory, are the prose that explains *why* those cases say what they say
— every claim below is checked against the current Swift source, with a
citation, so the Rust implementation is built against what the code
actually does rather than against a plausible-sounding description of it.

## Scope

**Shared (must match):**
- Plan-tier detection logic (`account-schema.md`, this file's "Plan tier"
  section).
- The `UsageData` decode shape, including how the Fable window is derived
  (this file's "The Fable window" section).
- Burn-rate levels and thresholds, and the resulting sort order (this
  file's "Burn-rate levels" and "Sort order" sections).
- The helper CLI's three subcommands — `decrypt`, `usage`, `sync` — as
  externally observable behaviour (input/output shape, exit codes, stderr
  text): `helper-cli.md`.
- The `Account` JSON schema and its two backward-compatibility defaults:
  `account-schema.md`.
- The `usage_logs` SQLite schema and its compression semantics:
  `usage-log.md`.

**Platform detail (deliberately free to differ):**
- **Cookie decryption.** How a session key is extracted from the browser's
  cookie store is macOS/Chromium-specific (`apps/macos/Shared/ChromeCookieService.swift`
  uses PBKDF2-SHA1 + AES-128-CBC keyed off the Keychain-held Chrome Safe
  Storage password). A Linux implementation reading a different browser's
  cookie store needs none of this.
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

Each file under `contract/cases/` (added in later tasks) is a JSON array of
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

## Plan tier

Plan tier is derived **only** from `GET /api/organizations` (specifically
`UsageAPIService.fetchOrganizations`, `apps/macos/Shared/UsageAPIService.swift:49-73`)
— **never** from the usage endpoint. This directly contradicts the current
top-level `CLAUDE.md`, which says plan tier is detected "from `extra_usage`
response field" — that line is wrong and this file is the corrected record.

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

If a `weekly_scoped` / `Fable` entry exists but its own `percent` is
missing, utilization defaults to `0` (it is *not* treated as "no Fable
window" — that only happens when no matching entry exists at all, or the
`limits` array itself is absent, in which case `fable` decodes to `nil` and
the UI hides the gauge).

`seven_day_sonnet` is a field the current API no longer returns. The
decoder's `RootKeys` (`apps/macos/Shared/UsageData.swift:60-64`) declares no
case for it, so if a `seven_day_sonnet` key were ever present in a response
it would be dropped the same way `JSONDecoder` drops any unrecognized key
under a keyed container — there is no explicit skip/guard for it in the
code, it simply isn't looked for. This directly contradicts top-level
`CLAUDE.md`'s claim that `UsageData` has "5-hour, 7-day, and Sonnet
windows" — the third window is Fable, not Sonnet, and has been since the
`fable` field replaced it; this file is the corrected record.

A third place carrying the same stale claim, found while writing this
document: `README.md:92` ("**S** — 7-day Sonnet-specific utilization (Max
plans only)"), describing a row the bash CLI (`cli/claude-dashboard-cli`)
renders. See `helper-cli.md` for why that row no longer renders in
practice.

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
