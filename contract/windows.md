# Windows behaviour

Platform-scoped, for the Windows app (`apps/windows/`) and its native-messaging
bridge. Account **business rules** — plan detection, dedupe, org selection,
burn rate, the Fable window — are unchanged and stay governed by the other
`contract/` files; they now run on Windows too (the Swift-mirroring `core`
crate's tests execute under `cargo test` on `windows-latest`). This file covers
only what is Windows-specific.

## Data paths

| File | Location |
|---|---|
| Accounts store | `%APPDATA%\claude-dashboard\accounts.json` |
| Usage log DB | `%LOCALAPPDATA%\claude-dashboard\usage_logs.db` |
| Extension sources | `%APPDATA%\claude-dashboard\extension-sources.json` (beside the store) |
| Store write lock | `%APPDATA%\claude-dashboard\accounts.json.lock` |

Two processes write `accounts.json` on Windows (the app and the bridge). A
writer holds an exclusive lock on `accounts.json.lock` across its whole
read-modify-write, then writes a temp file and renames it over the store.

## At-rest session-key protection

Session keys in the store are sealed with the Windows Data Protection API
(`CryptProtectData`, current-user scope) and base64-encoded — the same one
base64-string wire shape as the Unix scheme, but **not** byte-compatible: a
store is useless on another machine or OS, and `sessionKey` was never portable
(see `account-schema.md`, "sessionKey is not portable").

## Cookie formats

Chromium cookies on Windows carry a 3-byte version tag:

- `v10` / `v11`: AES-256-GCM, body = `nonce(12) || ciphertext || tag`, keyed by
  `Local State`'s `os_crypt.encrypted_key` (base64, `"DPAPI"` prefix, then a
  DPAPI blob). Decoded by `core::cookie::win::decode_value`.
- `v20`: app-bound encryption (Chrome 127+), whose key is bound to the browser
  through a SYSTEM service. **Never opened.** It reports
  `CookieError::AppBoundEncrypted`, and the app routes the user to the browser
  extension instead of scanning.

A schema-version >= 24 plaintext carries a `SHA256(host_key)` prefix, stripped
exactly as on the other platforms.

## Native messaging

Host name: `com.claude_dashboard.bridge`. The browser launches a fresh host
process per message; the host reads one message, acts, replies, and exits.

Framing: a 32-bit little-endian length followed by that many bytes of UTF-8
JSON (the Chrome native-messaging convention).

**Message in** (extension → host):

    { "type": "sessionKey", "installId": "<uuid>", "browser": "chrome|edge|brave",
      "sessionKey": "<key>" }

**Reply out** (host → extension):

    { "ok": true, "email": "<email or null>" }
    { "ok": false, "error": "<code>", "message": "<human text>" }

Error codes: `bad_message` (unparseable, wrong type, or missing install id),
`no_key` (blank session key), `muted` (this install's account was deleted),
`rejected` (the session key was not accepted by `/api/account`), `no_chat_org`
(the account has no organization with chat access), `store` (an account- or
sources-store read/write failed).

The session key never appears in a reply, a log line, or any error message.

Processing order (the bridge): parse and validate (no I/O) → muted check
(before the network) → fetch `/api/account` then `/api/organizations` (never
under the lock) → under the store lock: reload sources, re-check muted, load
accounts, apply the key (add or repair, via `core::key_intake`), save accounts,
bind the install, save sources → after the lock: notify the app.

## Registration

The host manifest (`com.claude_dashboard.bridge.json`) is written next to the
bridge exe with an absolute `path`, and registered under the per-user key each
browser reads:

- `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.claude_dashboard.bridge`
  (Chrome, Brave, Arc)
- `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.claude_dashboard.bridge`
  (Edge)

`allowed_origins` lists the extension's fixed id
(`chrome-extension://cadpjcfajhlgdaipepkdojcehfmkkedh/`), which is fixed by the
`key` field in the extension manifest. `apps/windows/scripts/register-dev-host.ps1`
does this for local development.

## extension-sources.json

    {
      "bindings": { "<installId>": { "accountId": "<id>", "browser": "chrome" } },
      "muted": ["<installId>", ...]
    }

`bindings` records which extension install feeds which account, so the app can
tell an extension-sourced account from a hand-pasted one. **Muted rule:**
deleting an extension-sourced account adds its `installId` to `muted`; the host
then ignores that install's keys (reply `muted`) so a deleted account is not
silently re-added. Settings offers an unmute. This file is kept out of
`accounts.json` so the cross-platform account schema stays unchanged;
extension-sourced accounts are stored with `source: "manual"`.

## App reload pipe

The app serves a named pipe `\\.\pipe\claude-dashboard`; the bridge writes
`reload\n` after a successful store change so a running app refreshes. A missing
pipe means the app is not running and is not an error.

## App shell

- **Single instance:** the app holds the named mutex
  `Local\ClaudeDashboardSingleInstance` for its lifetime; a second launch fails
  to acquire it and exits.
- **Reload pipe consumer:** the app is the *reader* of `\\.\pipe\claude-dashboard`
  and refreshes on each `reload` line; the bridge (sub-project 1) is the writer.

## Manual smoke test

End-to-end extension → bridge → store is verified by hand (the automated
`roundtrip` test covers framing and the rejected path; the happy path is
covered by the handler's unit tests):

1. Build the bridge, then run `apps/windows/scripts/register-dev-host.ps1`
   with the built exe path and the fixed extension id.
2. Load `apps/windows/extension/` as an unpacked extension.
3. Open claude.ai logged in. The extension popup should show
   "Synced <email>", and the account should appear in `accounts.json`.
