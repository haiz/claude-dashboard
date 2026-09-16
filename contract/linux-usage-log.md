# Linux usage-log file format

Platform-scoped, and narrower than `contract/usage-log.md`: that document pins
the *column semantics and compression policy* every implementation must obey
and is explicit that the storage engine is not contract. This one pins the
single concrete on-disk encoding the Linux processes share, because two
implementations in two languages now read and write the same file.

Path: `$XDG_DATA_HOME/claude-dashboard/usage-log.json` (default
`~/.local/share/claude-dashboard/usage-log.json`).

## Encoding

    { "version": 1, "rows": [ {"id": 1, "aid": "<accountId>", "w": 0,
                               "rat": 1789430000, "t": 1789420000,
                               "u": 500, "lim": 0} ] }

- `aid` is the account id **as a string** — the store's own id, not the integer
  indirection the SQLite schema uses.
- `w` is the window: `0` five-hour, `1` seven-day, `3` Fable. `2` is the
  retired Sonnet window: inert history, never written again.
- `rat` and `t` are Unix **seconds**, truncated toward zero.
- `u` is `round(utilization * 100)`, rounding half away from zero.
- `lim` is `0` or `1`.
- `id` is a per-document counter starting at 1.

## Writer obligations

The insert-time compression policy of `contract/usage-log.md` applies
unchanged, and runs **before** each insert.

`contract/cases/linux-usage-log.json` pins one exact document: `recordings` is
the input sequence, `serialized` is the byte-exact output every writer must
produce from it. It is generated from `apps/linux/lib/usageLog.js`, the
reference implementation, and both the JS and the Rust writer are tested
against it.
