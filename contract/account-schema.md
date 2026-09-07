# Account schema

Source: `apps/macos/Shared/Account.swift`. This is the JSON shape persisted
by both `AccountStore` (main app,
`apps/macos/ClaudeDashboard/Services/AccountStore.swift:44-55`) and
`HelperAccountStore` (privileged helper,
`apps/macos/Helper/HelperAccountStore.swift:8-23`) — same struct, same
encoder, two callers. *Where* that JSON is stored (`UserDefaults` on macOS)
is platform detail; the shape below is contract, and so is who may read the
stored bytes — see "Store file permissions" at the end.

## Fields

`Account` (`apps/macos/Shared/Account.swift:27-53`):

| Field | Type | Optional | Notes |
|---|---|---|---|
| `id` | UUID | no | |
| `name` | String | no | |
| `email` | String | yes | |
| `chromeProfilePath` | String | no | Despite the name, holds the profile path for whichever `browser` the account came from — not Chrome-specific. |
| `chromeProfileName` | String | yes | Same naming note as above. |
| `orgId` | String | yes | `isConfigured` (line 50-52) is `orgId != nil`. |
| `accountUuid` | String | yes | The Claude account's own uuid, from `GET /api/account`. The identity key for dedupe. Absent on records written before this field existed; backfilled on the next successful refresh. `orgId` is **not** an identity and must never be compared as one. |
| `sessionKey` | String | yes | Ciphertext — see "sessionKey is not portable" below. |
| `browser` | `Browser` | no (defaults to `.chrome` on decode) | See "Backward compatibility". |
| `plan` | `AccountPlan` | no | |
| `lastSynced` | Date | yes | |
| `status` | `AccountStatus` | no | |
| `isPinned` | Bool | no (defaults to `false` on decode) | See "Backward compatibility". |
| `source` | `AccountSource` | no (defaults to `browser` on decode) | `browser` or `manual`. A manual record's key was pasted by the user: `chromeProfilePath` is `""`, `chromeProfileName` is absent, and `browser` is meaningless. Consumers key on this field alone; an empty `chromeProfilePath` is not a secondary signal. |

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

`AccountSource` wire values are `"browser"` and `"manual"`.

**Downgrade is one-way.** An older build reads a record carrying `source`
without failing, because `chromeProfilePath` is still present. But both writers
re-serialize the whole array (`HelperAccountStore.saveAccounts`,
`Account::to_json_array`) and both drop unknown keys on encode: Swift's
synthesized `encode(to:)` follows the explicit `CodingKeys`, and the Rust derive
has no catch-all. The first write from an older build therefore strips `source`,
and the record reads afterwards as browser-backed with an empty profile path.
Usage polling still works; re-sync reports opening a profile named `''` until the
key is pasted again.

## Backward compatibility

The custom decoder (`Account.swift:58-81`, `init(from:)` at lines 64-80)
enforces three defaults for keys that did not exist in older persisted JSON:

1. **Missing `browser` defaults to `.chrome`** (line 74:
   `try c.decodeIfPresent(Browser.self, forKey: .browser) ?? .chrome`). This
   is exercised by
   `apps/macos/ClaudeDashboardTests/AccountCodableTests.swift:7-22`
   (`testDecodeLegacyJSONDefaultsToChrome`), which decodes a JSON object with
   no `"browser"` key and asserts `account.browser == .chrome`. This dates
   from before multi-browser support existed.
2. **Missing `isPinned` defaults to `false`** (line 78:
   `try c.decodeIfPresent(Bool.self, forKey: .isPinned) ?? false`). Verified
   directly from source; there is no dedicated unit test for this specific
   default in the current test suite — the `browser` and `source` defaults
   each have one, this one does not.
3. **Missing `source` defaults to `.browser`** (line 79:
   `try c.decodeIfPresent(AccountSource.self, forKey: .source) ?? .browser`).
   Every record written before the pasted-key feature existed is
   browser-backed. Exercised by
   `apps/macos/ClaudeDashboardTests/AccountSourceTests.swift`
   (`testALegacyRecordWithNoSourceDecodesAsBrowser`), which decodes a JSON
   object with no `"source"` key and asserts `account.source == .browser`.

All three defaults exist for the same reason: a field was added to `Account`
after accounts were already persisted on users' machines, and decoding must
not throw on the old shape.

The file carries its own warning about this decoder, at
`Account.swift:55-57` (translated from the original Vietnamese comment):

> Custom decode for compatibility with old JSON (missing key "browser" →
> `.chrome`). NOTE: keep `CodingKeys` and `init(from:)` in sync with every
> stored property of `Account`; adding a new property and forgetting to
> update here will silently lose data on round-trip.

This is not a stylistic preference — `CodingKeys`
(`Account.swift:59-62`) is a private, hand-maintained enum that does not
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

## Store file permissions

The account store is readable and writable by its owning user only. This is
contract, not platform detail: every field above travels in that one blob,
`sessionKey` included, and the at-rest encryption around `sessionKey` does
not make the blob safe to leave world-readable.

Why it does not: both platforms' at-rest schemes bind ciphertext to a
*host*, not to a *user*. macOS derives its key from `IOPlatformUUID`; Linux
derives its from the contents of `/etc/machine-id`
(`apps/linux/core/src/store.rs`), which is mode `444` on a stock systemd
install. A second local user who can read the store can also read the key
material it was derived from, so the ciphertext buys nothing against them.
File permissions are what separate two users; the encryption is what
separates two machines. Neither substitutes for the other.

How each implementation satisfies it:

- **macOS** — no explicit call. `UserDefaults` persists through `cfprefsd`,
  which writes `~/Library/Preferences/<suite>.plist` mode `600` inside a
  directory that is itself mode `700`. Confirmed by inspection on
  2026-09-07: `com.claude-dashboard.app.plist` is `600`,
  `~/Library/Preferences` is `700`.
- **A port that persists to a flat file** — Linux's
  `$XDG_CONFIG_HOME/claude-dashboard/accounts.json` — must do it explicitly,
  because the process umask decides otherwise and the usual `022` yields
  `644`. Three requirements:
  1. Create the file mode `0600`. Setting the mode *at creation* rather than
     after the write is what keeps a newly written store from existing
     world-readable even briefly.
  2. Create the containing `claude-dashboard` directory mode `0700`. That
     leaf directory only: `$XDG_CONFIG_HOME` itself holds the user's wider
     configuration and is not this contract's to narrow.
  3. Repair a store that is already `644`, written by a version predating
     this rule. Create-time mode alone never repairs one, because the mode
     applies to the file being created and not to a destination being
     rewritten in place. Satisfying the next section is what settles this:
     the replacement carries its own `0600` onto the destination, so no
     separate `chmod` step is needed and none should be added back.

Out of scope: the usage-log database (`contract/usage-log.md`). It holds
utilization percentages, reset timestamps and account ids, no credential, so
it stays at the platform default rather than acquiring a rule this document
would have to keep in sync.

## Store writes replace, never rewrite in place

A save publishes the new store as one indivisible step. A reader never
observes a half-written store, and a process that dies mid-save leaves the
previous store exactly as it was. The failure mode this rules out is losing
*every* account, not losing the last change.

Why it is contract and not platform detail: the store is the only record of
each account's `accountUuid`, `orgId` and `sessionKey`. Nothing on the server
identifies which Claude accounts a given machine had added, so a truncated
store cannot be re-synced from anywhere — it is recovered by the user
finding and re-adding every account by hand. A stale store loses one
change; a torn one loses the whole set. The two are not the same kind of
failure, and only the second is worth a rule.

How each implementation satisfies it:

- **macOS** — no explicit call. `UserDefaults` persists through cfprefsd,
  which publishes a *new* file on every write rather than rewriting the
  existing one: measured on 2026-09-07 against an isolated suite, the
  plist's inode changed on each of three consecutive writes
  (`491370913`, `491370923`, `491370924`, `491370925`), at mode `600` every
  time. That measurement shows replacement, which is what this section
  requires; it does not go further and identify cfprefsd's mechanism as
  `rename` specifically.
- **A port that persists to a flat file** — write the bytes to a temporary
  file in the **same directory** as the store, then `rename(2)` it over the
  destination. Same directory because `rename` is only atomic within one
  filesystem, so a temp under `/tmp` or `$TMPDIR` fails the moment the two
  are separate mounts. Truncating the destination and rewriting it is ruled
  out, and so is unlinking it first: both open a window in which the store
  on disk is partial or absent.

Durability is a separate question from atomicity, and the line between them
is deliberate: `fsync` the temporary file before the rename, because
otherwise a power loss just after the rename can publish a file whose bytes
never reached the disk — the same "lost every account" outcome by a
different route. Do **not** `fsync` the directory to make the rename itself
durable: a power loss that forgets the rename leaves the previous store,
which is the safe side of this rule to land on.

## An unreadable store is not an empty store

Loading the store has three outcomes, not two: it holds accounts, it does not
exist yet, or it exists and cannot be parsed. The third must never reach the
rest of the program disguised as the second.

Why it matters more than it looks: a writer that reads an unreadable store as
an empty one destroys it. It loads nothing, adds whatever it was asked to
add, and writes the result over the bytes it could not parse. By the section
above those bytes are the user's only copy of every account's `accountUuid`,
`orgId` and `sessionKey`, so the cost of the confusion is the whole set.
"Store writes replace, never rewrite in place" removes one way a store
becomes unparseable; it does not remove the others, disk exhaustion and
version skew among them.

The rule, for every code path that writes:

1. Distinguish a parse failure from an absent store. An absent store is
   genuinely no accounts and needs nothing.
2. Never overwrite bytes that failed to parse. Move them aside first, to a
   name derived from the store's own, marked unreadable and carrying a
   timestamp so that a second failure cannot overwrite the copy the first one
   kept. After the move the store is absent, not corrupt, and writing is
   safe.
3. Proceed from an empty set and tell the user where the bytes went. A CLI
   says it on stderr, exactly:

       Could not read the account store. The unreadable copy is kept at
       <location>; this run starts from no accounts.

   printed as one line, where `<location>` names wherever step 2 put the
   bytes: a filesystem path where the store is a file, the key where it is a
   preferences domain. An implementation with no such channel still owes
   step 2; how it surfaces the message is its own business.
4. Distinguish a parse failure from an I/O failure, and quarantine only the
   parse failure. An unreadable directory or a permission error can leave a
   perfectly good store on disk, and moving it would not help; that case is
   an error, and a writer that hits it writes nothing at all.

Read-only paths are excluded on purpose. Moving the store aside is a write,
and a command that only reads has nothing to protect by doing it, so `decrypt`
keeps reporting an unreadable store the same way it reports an empty one (see
`contract/helper-cli.md`'s "`decrypt`" section). That is a diagnosability
gap, not a data-loss one.

