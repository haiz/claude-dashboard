# Claude Code account switch (macOS)

Date: 2026-10-08
Status: approved design, awaiting implementation plan
Scope: `apps/macos/ClaudeDashboard` only. Linux, Windows and the helper CLI are untouched.

## Goal

Let a user who rotates several claude.ai accounts through **one** Claude Code config
directory (`~/.claude`) switch the account Claude Code uses with one click, without a
browser sign-in. Each account signs in with `/login` once; after that the app swaps
credentials. Success: an account needs a browser sign-in only when its refresh token
reaches its fixed expiry (about a month), not every day.

The user keeps a single config directory on purpose: sessions and context
(`~/.claude/projects`) must stay shared across accounts. Per-account
`CLAUDE_CONFIG_DIR` directories are therefore not an option.

## Why the browser step happens today

`/login` replaces the credential in `~/.claude`, discarding the previous account's
refresh token. Switching back needs a new OAuth grant, and claude.ai demands a recent
sign-in for a grant (`/login?reauth=1`, `reason=elevated_auth`,
`auth_kind=session_stale_relogin`; anthropics/claude-code#89812, #93014, #78946). The
freshness window is about 24 hours (not documented; measured from Chrome history, and
reported as "roughly daily" upstream). Refreshing an existing token goes through the
token endpoint, not the browser, so it never meets that check. Keeping every account's
refresh token alive and swapping it in avoids the grant entirely.

## Facts this design rests on

Observed on Claude Code 2.1.294, macOS, 2026-10-08 (spike, all accounts restored):

1. The credential lives in the login Keychain, service `Claude Code-credentials`,
   account `$USER`, as JSON with `claudeAiOauth` (`accessToken`, `refreshToken`,
   `expiresAt`, `refreshTokenExpiresAt`, `scopes`, `subscriptionType`,
   `rateLimitTier`) and, alongside it, `mcpOAuth` (other MCP servers' tokens).
   `~/.claude/.credentials.json` is only a fallback when the Keychain write fails.
2. `~/.claude.json` holds `oauthAccount` (email, org, account UUID): identity metadata,
   not a credential. `ClaudeCodeAccountDetector` already reads it.
3. An account idle for ~17 hours refreshed silently. Every refresh **rotates** the
   refresh token; the access token lives ~8 hours.
4. `refreshTokenExpiresAt` does **not** move on refresh. It is a fixed deadline set by
   the grant.
5. A running `claude` process re-reads the Keychain (30-second read cache). Swapping a
   valid credential of another account into the entry mid-session made the session
   continue on that account, refresh it, and write the rotated token back into the entry.
6. Swapping in an invalid credential made the session fail with `OAuth session expired
   and could not be refreshed`, and Claude Code then **blanked** the entry
   (`accessToken`/`refreshToken` empty, `expiresAt` 0).

Not verified: a session currently blocked by a usage limit (HTTP 429). Fact 5 implies
its next request after the cache window uses the swapped token.

## Risks the design must handle

- **Stale vault copy.** The active account's token keeps rotating inside the Keychain
  entry. Writing an older copy back kills that account (fact 6).
- **Refresh during a switch.** If a session refreshes account A between the app saving
  A and writing B, A's newest token is lost.
- **Two holders of one account.** If the same account is also used through another
  config directory (for example a `claude-backend` alias), both copies share one
  rotating refresh-token chain; whichever refreshes second dies. Documented as a user
  rule, not handled in code.

## Components

All four are protocols with a real implementation and an in-memory fake.

1. **`ClaudeCodeKeychain`**: read and write the `Claude Code-credentials` entry.
   Writing replaces only the `claudeAiOauth` key and preserves every other key
   (`mcpOAuth` must survive a switch).
2. **`CredentialVault`**: the app's own store, one Keychain item per account (service
   `ClaudeDashboard.cc-vault`, account = `Account.id`). Each item holds the account's
   `claudeAiOauth` object and its `oauthAccount` object.
3. **`ClaudeConfigAccountWriter`**: replaces only the `oauthAccount` key of
   `~/.claude.json`, writing a temp file in the same directory and renaming it over the
   original. Extends the reading side of `ClaudeCodeAccountDetector`.
4. **`ClaudeCodeSwitcher`**: orchestrates capture and switch. Owned by
   `DashboardViewModel`, injected so tests replace it.

## Data flow

### Capture (every `refreshAll`)

Runs next to the existing `activeClaudeCodeEmail = ccDetector.activeEmail()`.

1. Read the email in `~/.claude.json`; find the `Account` whose `email` matches,
   case-insensitively. No match: stop.
2. Read `claudeAiOauth` from the Keychain entry. If `refreshToken` is empty (blanked by
   Claude Code, fact 6): stop, and mark the account "needs `/login`".
3. If it differs from the vault copy, write it to the vault with the current
   `oauthAccount`.

A user therefore runs `/login` once per account; the next refresh captures it.

### Switch to account B (A active)

1. Refuse if the vault has no copy of B, or B's `refreshTokenExpiresAt` has passed.
   Message: "Run /login once for B".
2. If A's access token expires in under 5 minutes (a refresh is imminent), wait: poll
   every 3 seconds, up to 30 seconds, until `expiresAt` moves forward. Timeout: fail
   with "Claude Code is refreshing, try again".
3. Capture A (the capture steps above, run now).
4. Write B's `claudeAiOauth` into the Keychain entry, preserving other keys.
5. Write B's `oauthAccount` into `~/.claude.json`.
6. Read both back to verify, update the active badge, and show "Switched to B. Running
   sessions pick it up within ~30 s."

## Error handling

- Step 4 fails: nothing has changed; report the error, A stays active.
- Step 5 fails: write A's `claudeAiOauth` back into the Keychain entry (rollback), then
  report. Token and identity never disagree.
- Step 6 verification fails: report it; do not retry automatically.
- `~/.claude.json` is also written by running Claude Code processes. The
  read-modify-rename window is kept short; losing one concurrent Claude Code write is a
  known, accepted risk.
- Every switch attempt (from, to, outcome) is logged with the codebase's existing
  `print("[ClaudeCodeSwitcher] ...")` diagnostic convention, never including token
  values. The Command Log is not used: its schema models process runs (exit code,
  timeout), not switches.

## UI

- `AccountCard` and `AccountPane` gain a **Switch** button. It is hidden on the account
  that is already active (the existing badge marks it).
- Disabled, with a tooltip, when the vault has no copy ("Run /login once in Claude Code
  with this account") or the copy is past `refreshTokenExpiresAt` ("Login expired, run
  /login").
- The four intentional `AccountCard` gauge details stay unchanged.

## Testing

- Unit tests in `ClaudeDashboardTests` drive all four components through fakes and a
  temp `~/.claude.json`: `mcpOAuth` preserved; no capture from a blanked entry; wait
  then proceed when the token is near expiry; rollback when the config write fails;
  refusal for a missing or expired vault copy; case-insensitive email match.
- The test host is the real app and runs `refreshAll` at startup. Under XCTest the
  real Keychain-backed components must be replaced by no-ops, the way `AppDefaults`
  diverts defaults. A test proves the real Keychain is not touched.
- Manual check after build: repeat spike test 3 through the button. A running `claude`
  session moves to the other account, and both accounts still work afterwards.

## Out of scope

- Linux and Windows (`.credentials.json`); a `contract/` section can follow when a port
  needs it.
- Config directories other than `~/.claude`.
- The app refreshing idle accounts itself.
- Detecting the same account in another config directory.
