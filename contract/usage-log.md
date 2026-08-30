# Usage log schema

Source: `apps/macos/ClaudeDashboard/Services/UsageLogStore.swift`. This is a
local time-series log of polled usage percentages, stored in SQLite. **The
database file itself is not contract** — a Rust implementation may use a
different engine, file layout, or even a different persistence mechanism
entirely. What is shared is the **column semantics** and the **compression
policy applied on insert**, because both determine what values a query
against equivalent history must return. Byte-identical `.db` files across
implementations are not required or expected.

## Tables

`createTables()` (`UsageLogStore.swift:316-340`) creates two tables:

```sql
CREATE TABLE IF NOT EXISTS accounts_map (
    aid INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL UNIQUE
)

CREATE TABLE IF NOT EXISTS usage_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    aid INTEGER NOT NULL,
    w INTEGER NOT NULL,
    rat INTEGER NOT NULL,
    t INTEGER NOT NULL,
    u INTEGER NOT NULL,
    lim INTEGER DEFAULT 0
)

CREATE INDEX IF NOT EXISTS idx_logs_lookup ON usage_logs(aid, w, rat, t)
```

`accounts_map` exists only to turn a 16-byte `UUID` account id into a small
integer `aid` for compact storage/indexing in `usage_logs`
(`resolveAccountId`/`lookupAccountId`, lines 278-303) — this indirection is
itself an implementation detail; what matters is that log rows are keyed by
account.

## `usage_logs` columns

| Column | Meaning | Unit / encoding |
|---|---|---|
| `aid` | Foreign key into `accounts_map`, identifying the account | integer |
| `w` | Which usage window this row logs | `UsageWindow.rawValue` — see below |
| `rat` | "reset-at" — the `resetsAt` timestamp reported by the API for this window at record time | Unix seconds, integer, truncated (see `record`, line 25: `Int64(resetsAt.timeIntervalSince1970)`) |
| `t` | When this row was recorded (poll time) | Unix seconds, integer, truncated (line 26: `Int64(Date().timeIntervalSince1970)`) |
| `u` | Utilization at record time | integer percent **times 100** — see below |
| `lim` | Whether the account was rate-limited at record time | `1` or `0` |

All of this is set in `record(accountId:window:resetsAt:utilization:isLimited:)`
(`UsageLogStore.swift:22-44`).

### `w` — window identifier

`UsageWindow` (`apps/macos/ClaudeDashboard/Models/UsageLogModels.swift:4-18`):

| Case | Raw value (`w`) |
|---|---|
| `.fiveHour` | `0` |
| `.sevenDay` | `1` |
| `.fable` | `3` |

Rawvalue `2` is **deliberately unused** — it belonged to a retired `.sonnet`
case. The comment at `UsageLogModels.swift:7-8` states this explicitly:
`.fable` is given rawValue `3`, not `2`, "so Fable logs never collide with
the retired Sonnet window that persisted rows at rawValue 2 in older
databases." A Rust reader of an existing database (or a fresh one seeded
with the same convention) must treat `w = 2` as belonging to the extinct
Sonnet window, not reassign it to Fable or any other window — old rows with
`w = 2` are inert history, not a value a writer should ever produce again.

### `u` — utilization encoding, and how it differs from `t`/`rat`

`u` is stored as `Int64(round(utilization * 100))` (`UsageLogStore.swift:27`)
— e.g. a 45.5% utilization is stored as `4550`. On read, it is divided back
by 100.0 to recover the `Double` (e.g. `logs(...)`, line 80:
`Double(sqlite3_column_int64(stmt, 2)) / 100.0`).

**This uses `round()`, not truncation.** This is a deliberate contrast with
the timestamp columns (`t`, `rat`) and with `README.md`'s "Timestamps"
rule: `Int64(someDouble)` (used for `t` and `rat`) truncates toward zero,
but `u`'s encoding explicitly calls `round()` before the `Int64` cast. A
Rust port must round `utilization * 100` to the nearest integer (round-half
rule matching Swift's `round()`, which rounds half away from zero), not
truncate it — truncating would silently shift every stored utilization
value downward by up to 0.99 (e.g. 45.999% truncating to 4599 vs. round to
4600 is a smaller drift, but 45.005 rounds to 4501 vs. truncates to 4500 —
the two encodings diverge on any fractional-percent boundary).

### `lim` — the `isLimited` flag

Stored as `Int32` `1`/`0` (line 28, 42), read back as `!= 0`
(`logs`, line 81). This is a plain boolean projection.

## Compression policy (applied on insert)

`applyCompression(aid:w:rat:u:)` (`UsageLogStore.swift:248-274`) runs
**before** every insert in `record(...)` (called at line 30). It is a
run-length "keep first and last of a plateau" compression, scoped to an
exact `(aid, w, rat)` triple:

1. Query the two most recent existing rows for this exact `(aid, w, rat)`,
   ordered by `t DESC LIMIT 2` (lines 249-256).
2. If there are at least two such rows, **and** their `u` values are equal
   to each other, **and** that shared value equals the `u` about to be
   inserted (lines 264-266): delete the more-recent of those two existing
   rows (line 272, "delete most recent (middle)" — it becomes the middle
   entry of the run once the new row is inserted).
3. Otherwise, do nothing; the new row is inserted normally afterward.

Effect: for a run of 3 or more consecutive polls at the same `(aid, w, rat)`
that all report the same rounded utilization, only the **first and last**
row of that run survive — every intermediate identical row is deleted as
each new identical value arrives. A value change ends the run; the next
value starts a fresh run and is never compressed away. This is verified by
`apps/macos/ClaudeDashboardTests/UsageLogStoreTests.swift`:
`testSmartCompression_threeIdenticalValues_keepFirstAndLast` (lines 37-52,
three identical inserts → 2 rows remain),
`testSmartCompression_fourIdenticalValues_keepFirstAndLast` (lines 68-79,
four identical inserts → still only 2 rows remain — the run keeps
compressing as it grows, not just the first triple),
`testSmartCompression_valueChanges_noCompression` (lines 54-66, three
distinct values → all 3 rows remain), and
`testSmartCompression_plateauThenChange` (lines 81-98, three identical then
one different → 3 rows remain: first+last of the plateau, plus the new
value).

Because compression is scoped to an exact `rat` match, it only engages when
consecutive polls report the *same* `resetsAt` for that window — which is
the normal case for a rolling window between resets, per the comment at
`resetCycles` (`UsageLogStore.swift:130-132`): "The API returns resetsAt =
now + 5h (rolling), so GROUP BY rat produces one 'cycle' per poll" — i.e.
`rat` drifts forward on almost every poll for the 5-hour window in practice,
so in real data the compression mostly fires for windows whose `resetsAt`
is more stable (7-day, Fable) or during a genuine utilization plateau within
one rolling cycle. A Rust implementation reproducing this schema must
implement the same three-condition check (row count ≥ 2, both existing
values equal, and equal to the incoming value) against the same key, not an
approximation like "collapse adjacent duplicates" without the exact-`rat`
scoping — the scoping is what keeps a window's actual reset transition (a
drop to a different `u`, or a new `rat`) from ever being compressed away.
