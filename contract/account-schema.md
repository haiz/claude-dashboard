# Account schema

Source: `apps/macos/Shared/Account.swift`. This is the JSON shape persisted
by both `AccountStore` (main app,
`apps/macos/ClaudeDashboard/Services/AccountStore.swift:44-55`) and
`HelperAccountStore` (privileged helper,
`apps/macos/Helper/HelperAccountStore.swift:8-23`) — same struct, same
encoder, two callers. *Where* that JSON is stored (`UserDefaults` on macOS)
is platform detail; the shape below is contract.

## Fields

`Account` (`apps/macos/Shared/Account.swift:16-38`):

| Field | Type | Optional | Notes |
|---|---|---|---|
| `id` | UUID | no | |
| `name` | String | no | |
| `email` | String | yes | |
| `chromeProfilePath` | String | no | Despite the name, holds the profile path for whichever `browser` the account came from — not Chrome-specific. |
| `chromeProfileName` | String | yes | Same naming note as above. |
| `orgId` | String | yes | `isConfigured` (line 35-37) is `orgId != nil`. |
| `accountUuid` | String | yes | The Claude account's own uuid, from `GET /api/account`. The identity key for dedupe. Absent on records written before this field existed; backfilled on the next successful refresh. `orgId` is **not** an identity and must never be compared as one. |
| `sessionKey` | String | yes | Ciphertext — see "sessionKey is not portable" below. |
| `browser` | `Browser` | no (defaults to `.chrome` on decode) | See "Backward compatibility". |
| `plan` | `AccountPlan` | no | |
| `lastSynced` | Date | yes | |
| `status` | `AccountStatus` | no | |
| `isPinned` | Bool | no (defaults to `false` on decode) | See "Backward compatibility". |

## Wire encoding of the non-string scalars

Both writers use a **bare `JSONEncoder()`** with no strategy overrides —
`AccountStore.persist()`
(`apps/macos/ClaudeDashboard/Services/AccountStore.swift:44-47`, the encode
at line 45) and `HelperAccountStore.saveAccounts(_:)`
(`apps/macos/Helper/HelperAccountStore.swift:17-23`, the encode at line 19).
Both readers use a bare `JSONDecoder()` (`AccountStore.swift:49-55` and
`HelperAccountStore.swift:8-15`). Nothing sets `dateEncodingStrategy`,
`dateDecodingStrategy`, or `keyEncodingStrategy` anywhere on this path, so
Foundation's defaults are the wire format, and two of the fields encode in a
way a Rust port will get wrong if it guesses:

| Field | Swift type | JSON type | Encoding |
|---|---|---|---|
| `id` | `UUID` | string | The **uppercase, hyphenated** 36-character form, e.g. `"3B8C3678-3A00-425C-8D22-22BCA37AE65B"`. Not lowercase, not compact. A Rust `Uuid` must serialise uppercase-hyphenated and must accept that form on read. |
| `lastSynced` | `Date?` | number or `null` | A **`Double` of seconds since 2001-01-01T00:00:00Z** — Foundation's `.deferredToDate` default, which encodes `Date.timeIntervalSinceReferenceDate`. It is **not** a Unix epoch and **not** an ISO8601 string, and it is fractional, not truncated. Convert with `unix_seconds = value + 978307200.0`. Absent or `null` means never synced. |

The remaining fields hold no surprises: `name`, `email`,
`chromeProfilePath`, `chromeProfileName`, `orgId`, `accountUuid` and
`sessionKey` are JSON strings (or absent/`null` where optional); `browser`, `plan` and `status`
are JSON strings carrying the raw values tabulated below; `isPinned` is a
JSON boolean.

**Do not confuse `lastSynced` with the case-file timestamp rule.**
`README.md`'s "Timestamps" section requires every expected instant in
`contract/cases/*.json` to be an integer Unix second truncated toward zero.
That rule is about the *test vectors*, and applying it to this field would
be wrong twice over — wrong epoch and wrong precision.

## Enum raw values (wire values, not display strings)

These are the exact strings that appear in persisted JSON and in the
`decrypt` helper output (`helper-cli.md`). A Rust implementation must encode
and decode these literal strings, not a paraphrase.

`AccountPlan` (`Account.swift:3-8`):
| Case | Raw value |
|---|---|
| `.pro` | `"Pro"` |
| `.max5x` | `"Max 5x"` |
| `.max20x` | `"Max 20x"` |
| `.max200` | `"Max"` |

The naming trap: `.max200`'s wire value is `"Max"`, not `"Max 200"` or
`"Max20x"` — it is the *fallback* case used when the tier is a consumer Max
account but the 5x/20x distinction is unknown (see `README.md`'s "Plan
tier" section, steps 4-5 of `detectPlanTier`). Do not infer a numeric tier
from this raw value.

`AccountStatus` (`Account.swift:10-14`): `"active"`, `"expired"`, `"error"`
(the enum's raw values are its unadorned case names — Swift's default
`String` raw-value synthesis).

`Browser` (`apps/macos/Shared/Browser.swift:6-10`): `"chrome"`, `"arc"`,
`"brave"`, `"edge"` (same default-synthesis rule — the enum has no explicit
`= "..."` per case, so each case's raw value is its lowercase name, which is
already lowercase here).

## Backward compatibility

The custom decoder (`Account.swift:43-65`, `init(from:)` at lines 49-64)
enforces two defaults for keys that did not exist in older persisted JSON:

1. **Missing `browser` defaults to `.chrome`** (line 59:
   `try c.decodeIfPresent(Browser.self, forKey: .browser) ?? .chrome`). This
   is exercised by
   `apps/macos/ClaudeDashboardTests/AccountCodableTests.swift:7-22`
   (`testDecodeLegacyJSONDefaultsToChrome`), which decodes a JSON object with
   no `"browser"` key and asserts `account.browser == .chrome`. This dates
   from before multi-browser support existed.
2. **Missing `isPinned` defaults to `false`** (line 63:
   `try c.decodeIfPresent(Bool.self, forKey: .isPinned) ?? false`). Verified
   directly from source; there is no dedicated unit test for this specific
   default in the current test suite (only the `browser` default has one).

Both defaults exist for the same reason: a field was added to `Account`
after accounts were already persisted on users' machines, and decoding must
not throw on the old shape.

The file carries its own warning about this decoder, at
`Account.swift:40-42` (translated from the original Vietnamese comment):

> Custom decode for compatibility with old JSON (missing key "browser" →
> `.chrome`). NOTE: keep `CodingKeys` and `init(from:)` in sync with every
> stored property of `Account`; adding a new property and forgetting to
> update here will silently lose data on round-trip.

This is not a stylistic preference — `CodingKeys`
(`Account.swift:44-47`) is a private, hand-maintained enum that does not
auto-include new stored properties, and `init(from:)` is a fully custom
initializer that does not fall back to memberwise decoding. A property added
to the struct without a matching `CodingKeys` case and a matching
`c.decode`/`c.decodeIfPresent` line in `init(from:)` will encode
successfully (the compiler-synthesized `Encodable` conformance still sees
it) and then silently vanish on the next decode — no error, no warning, just
data loss. Any Rust struct modeling `Account` must apply the same
discipline: every field addition needs an explicit, reviewed decode path,
not a derived one assumed to keep up automatically.

## `sessionKey` is not portable

`sessionKey`, when present, holds **ciphertext** produced by
`CryptoService.encrypt` (`apps/macos/Shared/CryptoService.swift:10-17`): a
base64-encoded AES-GCM sealed box, keyed by HKDF-SHA256 seeded from the
machine's `IOPlatformUUID` (`hardwareUUID()`,
`CryptoService.swift:39-53`) — see `README.md`'s "At-rest session-key
encryption" note. This value:
- Cannot be decrypted on any machine other than the one that encrypted it
  (there is no `IOPlatformUUID` on Linux at all, so the macOS scheme cannot
  even be reimplemented as-is on the Linux side — see `README.md`'s
  "Platform detail" section).
- Is never emitted in encrypted form by the `decrypt` helper subcommand —
  that subcommand's `sessionKey` output field is (attempted) plaintext, with
  a silent ciphertext-passthrough fallback on decrypt failure (see
  `helper-cli.md`).
- Must not be treated as a stable identifier or compared across machines or
  across a re-encryption; it is opaque bytes tied to one host's key.

**A third prose/code disagreement, found while writing this document:** the
top-level `README.md`'s "How It Works" list used to say "Stores session keys
securely in macOS Keychain". That was false, and it **has since been
corrected on this same branch** — the step now reads:

> Encrypts session keys with AES-GCM (key derived from the machine's
> hardware UUID) and stores them in the app's preferences

The reasoning behind that correction is kept here, because forks and older
checkouts still carry the Keychain wording. The dead code that made it
plausible — a `KeychainService` actor with `SecItemAdd`/`SecItemCopyMatching`
wrappers and a `sessionKey(for accountId:)` key-naming helper suggesting it
was built for exactly this purpose — sat unreferenced in the tree at
`apps/macos/ClaudeDashboard/Services/KeychainService.swift` until it was
deleted on this branch: `KeychainService.shared` was referenced nowhere
outside its own definition, and no `.save`/`.load` call site existed anywhere
in the app or its tests. The only Keychain
consumer in the codebase is `BrowserCookieService`, which only *reads* each
Chromium browser's own "Safe Storage" password (`SecItemCopyMatching`,
`BrowserCookieService.swift:261`) — it never writes anything. What the code
actually does is what this section already describes: `sessionKey` is
encrypted via `CryptoService.encrypt` (AES-GCM, key derived from
`IOPlatformUUID`) and persisted as a field inside the `Account` JSON blob in
`UserDefaults`, alongside every other account field — not written to the
Keychain via `SecItem*` at all. A Rust port must not model "the Keychain"
as part of the session-key storage contract; whatever at-rest scheme it
picks is platform detail (see `README.md`'s "Scope" section) — but it must
not read the old Keychain wording, in a fork or an older checkout, as
meaning `SecItem`-style secure-storage APIs are the mechanism to reproduce.
