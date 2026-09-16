# Linux daemon state file

Platform-scoped. macOS has no daemon and nothing here binds a Swift
implementation. It is in `contract/` because three independently-updated Linux
processes depend on it.

`claude-dashboard-helper watch` writes
`$XDG_DATA_HOME/claude-dashboard/state.json` (default
`~/.local/share/claude-dashboard/state.json`). The GNOME Shell extension and
the GTK app read it and never write it.

## Shape

    {
      "schemaVersion": 1,
      "polledAtMs": 1789432100000,
      "daemon": { "version": "1.17.2", "intervalSeconds": 120 },
      "activeClaudeCodeEmail": "someone@example.com",
      "accounts": [
        { "id": "...", "name": "...", "email": "...", "plan": "Max",
          "status": "active", "orgId": "...", "source": "browser",
          "chromeProfileName": null, "isPinned": false }
      ],
      "usage":  { "<accountId>": { ...verbatim `usage` response... } },
      "errors": { "<accountId>": "Could not refresh usage." },
      "fatal":  null
    }

`accounts[].id` is the **store's own account id** — the value `remove <id>`
takes and `list` prints. It is not derived from email: two accounts can share
an email across organisations.

`usage` values are the upstream API payload **verbatim**. The daemon does not
decode them. This is what keeps the Fable window — no top-level field, found in
`limits[]` by `scope.model.display_name`, read from `percent` rather than
`utilization` — solved in exactly one place, `apps/linux/lib/model.js`.

`errors` maps an account id to a human-readable message for that account's
failed fetch. `fatal` is non-null in exactly two cases, and they are not the
same situation:

- `"no-accounts"` — the account store was read successfully and holds zero
  accounts. Nothing is wrong; there is simply nothing to poll yet.
- `"store-unreadable"` — the account store could not be read or parsed at
  all, so this pass never learned what accounts exist. `accounts` is `[]`
  here too, but only because there is nothing else to project — it must not
  be read as "zero accounts" the way the first case is. See
  `contract/account-schema.md`'s "An unreadable store is not an empty store",
  the same distinction applied to what the daemon reports.

A partial failure — some accounts fetched, others errored — is never fatal:
`fatal` stays `null` and the per-account detail lives in `errors`.

## Rules

1. **Atomic write.** The daemon writes a sibling temporary file and `rename()`s
   it. A reader must never observe a partial document.
2. **Additive within a version.** Inside `schemaVersion: 1` the daemon may add
   fields; it may not remove one or change its meaning. Readers ignore fields
   they do not know.
3. **Unknown version means degraded.** A reader seeing a `schemaVersion` higher
   than it implements must fall back to its degraded presentation, not guess.
4. **Staleness is the reader's decision.** `polledAtMs` is the only input.
   Stale means older than `max(3 × daemon.intervalSeconds, 300)` seconds.

`contract/cases/linux-state.json` holds one literal document of each kind a
reader must handle.
