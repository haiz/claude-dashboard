# Windows Sub-project 3: Setup & Settings — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Let a user add and manage accounts in the Windows app: an Add Account wizard (browser extension / scan / paste), a Settings › Accounts pane (list, delete, re-sync, unmute), and a Settings › General pane (About, Auto Refresh, Launch at startup). The sidebar placeholders from sub-project 2 become real.

**Architecture:** Pure Rust. New testable `core` pieces: a settings store (`core::settings`), a Windows profile scanner that combines the sub-project-1 cookie primitives into detected sessions (`core::scan`, the classify half pure), and a launch-at-startup helper. The `apps/windows/app` crate gains the wizard and the two Settings panes (Slint), wiring those core pieces plus `core::key_intake` (paste) and the existing refresh loop (Auto Refresh interval). No account-schema change.

**Tech Stack:** Rust 1.98 (pinned); `slint`; `windows-sys` (registry for Launch-at-startup); `core` from `apps/linux/core`. Build + run on `windows-latest`.

**Spec:** `docs/superpowers/specs/2026-10-04-windows-app-design.md` (sub-project 3 of 6).

**Prior sub-projects (reuse, do not reinvent):**
- `core::store` (load/save accounts, `load_accounts_for_write`, `lock_store`), `core::key_intake::apply_session_key` (add/repair from a pasted or scanned key), `core::api` (`fetch_account`/`parse_account`/`fetch_organizations`), `core::extension_sources` (bindings + muted; `is_muted`, `bind`, `load`/`save`).
- Cookie scan primitives: `core::browser::discover_windows_profiles_under`, `core::browser::read_claude_cookie_db`, `core::cookie::win::profile_key`, `core::cookie::win::decode_value` (+ `CookieError::AppBoundEncrypted`).
- App: the sidebar with disabled Settings/Accounts placeholders, `selection`, `model::to_ui_row`, the refresh loop + nudge channel, `apps/windows/scripts/register-dev-host.ps1` and the extension (sub-project 1) for the extension tab.

Out of scope (later sub-projects): charts, Command Log, the auto-UPDATE mechanism + installer/release (sub-project 6). The General pane's "Updates" is a placeholder here (a "Check for updates" button may be disabled/"Coming soon").

## Global Constraints

- Toolchain 1.98.0 (pinned); app crate Windows-only; `core` additions stay plain Rust except Windows-only registry/DPAPI bits (`cfg(windows)` with non-Windows stubs so Linux CI builds).
- No account-schema change (`contract/account-schema.md`). Scanned/pasted accounts are stored exactly as `key_intake` writes them (`source: "manual"` for pasted; scanned browser accounts keep `source: "browser"` with the profile path — match how macOS stores a scanned account; see the macOS `SetupView`/`SyncCommand`).
- Deleting an extension-sourced account adds its `installId` to `extension_sources.muted` (so the extension can't silently re-add it); Settings offers unmute. This rule already exists in `contract/windows.md`.
- A `v20` (app-bound) cookie during a scan is reported as "use the extension", never force-decoded.
- Settings persist in `%APPDATA%\claude-dashboard\settings.json` (new `core::settings`); the refresh loop reads the Auto Refresh interval from it.
- Launch-at-startup is `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value name `ClaudeDashboard`, data = the quoted exe path; removing the value turns it off.
- All store writes (add/delete/mute) hold `core::store::lock_store()` and go through temp-file+rename (`save_accounts`), shared with the bridge.
- The session key never appears in logs, the UI, or error text.

## Review Focus

1. **Scan a profile whose cookie is `v20`** (Chrome 127+): the scan reports that profile as "needs the extension", does not error, and still returns any `v10` profiles found. → Task 2, test `scan_classifies_app_bound_as_needs_extension`.
2. **Paste an expired/invalid key**: the paste flow surfaces "key not accepted", writes nothing. → Task 3, test `paste_rejects_unaccepted_key_without_writing`.
3. **Delete an account that an extension install feeds**: it is removed AND its install muted so the next extension push does not re-add it; unmute re-enables. → Task 5, test `delete_mutes_the_extension_install`.
4. **Changing Auto Refresh interval** takes effect on the running loop without a restart, and an out-of-range/garbage stored interval falls back to a sane default. → Task 7, test `refresh_interval_clamps_and_defaults`.
5. **Launch-at-startup toggle** writes/removes exactly the one Run value with a correctly quoted path, and reads back its current state correctly. → Task 8, tests `startup_value_roundtrips` / `startup_command_is_quoted`.

## File / crate structure

```
apps/linux/core/src/
  settings.rs    (new)  Settings { auto_refresh_seconds, preferred_scan_browser, launch_at_startup }, load/save (%APPDATA%\...\settings.json), clamp/defaults
  scan.rs        (new)  ScannedSession, ProfileScanStatus (Found/AppBound/NoSession), classify + scan_windows_profiles() (cfg windows for the key read; pure classify tested)
  startup.rs     (new)  startup_command(exe)->String (quoted), + cfg(windows) enable/disable/is_enabled via HKCU Run
  lib.rs         (+ pub mod settings/scan/startup)
apps/windows/app/src/
  setup.rs       (new)  wizard controller: run a scan (core::scan) off-thread; paste (core::key_intake); extension-tab status (watch store/bindings); add selected detected accounts
  settings_accounts.rs (new) list/delete(+mute)/resync-all/unmute glue to core::store + extension_sources
  settings_general.rs  (new)  About text, Auto Refresh binding (-> settings + loop), Launch at startup (-> core::startup), Updates placeholder
  main.rs / refresh.rs  refresh loop reads core::settings interval (live)
  ui/*.slint     SetupWizard (3 tabs), AccountsSettings pane, GeneralSettings pane; sidebar Settings/Accounts items enabled + "Add Account" wired
contract/windows.md  (+ settings.json shape, Run key, scan-needs-extension behaviour)
CLAUDE.md            (+ settings/scan/startup modules; Setup & Settings)
.github/workflows/ci.yml  (already builds/tests the app crate; add core tests cover settings/scan/startup)
```

Tasks are added below in dependency order. TDD for core pieces (settings, scan classify, startup) and the paste glue; GUI tasks (wizard, panes) are build-run + the named pure helpers, verified by controller run-checks (screenshots).

---

### Task 1: core::settings (TDD)

**Files:** Create `apps/linux/core/src/settings.rs`; `lib.rs` `pub mod settings;` (with the ungated mods).

**Interfaces — produces:**
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "autoRefreshSeconds", default = "default_refresh")] pub auto_refresh_seconds: u64,
    #[serde(rename = "preferredScanBrowser", skip_serializing_if = "Option::is_none", default)] pub preferred_scan_browser: Option<String>,
    #[serde(rename = "launchAtStartup", default)] pub launch_at_startup: bool,
}
pub fn settings_path() -> PathBuf;              // store::accounts_path().with_file_name("settings.json")
pub fn load() -> Settings;                      // missing/corrupt -> Default (never errors; settings are non-critical)
pub fn save(s: &Settings) -> Result<(), String>;// temp-file + rename
impl Settings { pub fn effective_refresh_seconds(&self) -> u64; } // clamp to [MIN_REFRESH, MAX_REFRESH]
```
`default_refresh` = 60. MIN_REFRESH = 30, MAX_REFRESH = 3600. `effective_refresh_seconds` clamps (and maps 0/garbage to the default).

- [ ] **Step 1: failing tests** — a missing file loads `Settings::default()` (auto_refresh 60, no browser, launch false); round-trip through save/load in a temp dir (point `APPDATA` at it like the store tests do — reuse the env approach); `effective_refresh_seconds` clamps: 0 -> 60, 5 -> 30 (min), 99999 -> 3600 (max), 120 -> 120; a corrupt settings.json loads Default (NOT an error — settings are non-critical, unlike the account store).
- [ ] **Step 2-4:** run-fail, implement (serde struct; `load` returns Default on NotFound or parse error; `save` temp+rename), run-pass.
- [ ] **Step 5:** `cargo test -p claude-dashboard-core settings` + full `cargo test -p claude-dashboard-core` + `cargo clippy -p claude-dashboard-core --all-targets -- -D warnings`.
- [ ] **Step 6: commit** `feat(core): settings store for the Windows app`.

---

### Task 2: core::scan — detect Claude sessions in Windows profiles (TDD classify)

**Files:** Create `apps/linux/core/src/scan.rs`; `lib.rs` `pub mod scan;`.

**Port reference:** the macOS `SetupView` scan + the Linux helper `sync.rs::scan_profile` (same shape: read the profile's cookie DB, decrypt `sessionKey` and `lastActiveOrg`, a `v12`/`v20` short-circuits the profile). Here the key comes from `cookie::win::profile_key` and decode from `cookie::win::decode_value`.

**Interfaces — produces:**
```rust
pub struct ScannedSession { pub browser: Browser, pub profile_dir: String, pub display_name: String,
                            pub google_email: Option<String>, pub session_key: String, pub org_id: Option<String> }
pub enum ProfileScanStatus { Found(ScannedSession), AppBound, NoSession }   // AppBound => needs the extension
/// Pure: given a profile's decoded cookies, classify. Testable without files.
pub fn classify_profile(browser: Browser, profile_dir: &str, display_name: &str, google_email: Option<String>,
                        cookies: &[(String /*name*/, Result<String, CookieError>)]) -> ProfileScanStatus;
/// Windows: discover profiles under %LOCALAPPDATA%, read each DB, unwrap its key, decode the two cookies, classify.
#[cfg(windows)] pub fn scan_windows_profiles() -> Vec<ProfileScanStatus>;
```
`classify_profile`: if any cookie decode returned `Err(AppBoundEncrypted)` -> `AppBound`; else take `sessionKey`'s Ok value (and `lastActiveOrg`'s Ok value as `org_id`); `Found` when a session key is present, else `NoSession`. (Mirrors `scan_profile`'s per-cookie handling: a v20 short-circuits the profile to "needs extension".)

- [ ] **Step 1: failing tests** for `classify_profile` (Review Focus 1):
  - cookies with `sessionKey=Ok("sk..")` + `lastActiveOrg=Ok("org-1")` -> `Found` with those values.
  - any cookie `Err(CookieError::AppBoundEncrypted)` -> `AppBound` (even if a sessionKey Ok is also present — the profile is app-bound).
  - no `sessionKey` -> `NoSession`.
  - a non-AppBound decode error on an unrelated cookie is ignored (still `Found`/`NoSession` from sessionKey).
- [ ] **Step 2-4:** run-fail, implement `classify_profile` (pure) + `scan_windows_profiles` (cfg windows: `discover_windows_profiles_under(localappdata)`, `read_claude_cookie_db`, `profile_key(local_state)`, `decode_value` per cookie, then `classify_profile`). A profile whose key can't be unwrapped or DB can't be read -> `NoSession`. run-pass.
- [ ] **Step 5:** test + full core test + clippy.
- [ ] **Step 6: commit** `feat(core): scan Windows browser profiles for Claude sessions`.

---

### Task 3: core::startup — launch at startup (TDD)

**Files:** Create `apps/linux/core/src/startup.rs`; `lib.rs` `pub mod startup;`.

**Interfaces — produces:**
```rust
pub fn startup_command(exe_path: &str) -> String;     // the Run value data: the exe path wrapped in double quotes
#[cfg(windows)] pub fn enable(exe_path: &str) -> Result<(), String>;   // write HKCU\...\Run value "ClaudeDashboard"
#[cfg(windows)] pub fn disable() -> Result<(), String>;                // delete the value (absent is ok)
#[cfg(windows)] pub fn is_enabled() -> bool;                           // value present
```
Run key: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value name `ClaudeDashboard`.

- [ ] **Step 1: failing tests** (Review Focus 5):
  - `startup_command_is_quoted`: `startup_command(r"C:\Program Files\x\claude-dashboard.exe")` == `"\"C:\\Program Files\\x\\claude-dashboard.exe\""` (quoted so the space-bearing path is one argument).
  - `startup_value_roundtrips` (#[cfg(windows)]): enable(exe) -> is_enabled()==true and the stored value equals startup_command(exe); disable() -> is_enabled()==false; disable() again is Ok (absent is fine). Use the REAL Run key but a value name unique to the test (e.g. `ClaudeDashboardTest<pid>`) via a test-only inner fn that takes the value name, so the test never clobbers the real autostart entry; clean up in the test.
- [ ] **Step 2-4:** run-fail, implement (`RegSetValueExW`/`RegGetValueW`/`RegDeleteValueW` via windows-sys; factor the value-name into a private fn so the test can target a scratch name), run-pass.
- [ ] **Step 5:** test + full core test + clippy.
- [ ] **Step 6: commit** `feat(core): launch-at-startup Run-key helper for Windows`.

---

### Task 4: app apply-glue for paste & scanned keys (TDD)

**Files:** Create `apps/windows/app/src/setup.rs`; wire module in main.rs.

**Interfaces — produces (app crate):**
```rust
pub enum AddOutcome { Added(String /*name*/), Updated(String), Rejected(String /*reason*/), NoChatOrg, StoreError(String) }
/// Validate a pasted/scanned key over the network, then add/repair under the store lock.
pub fn add_from_session_key(session_key: &str) -> AddOutcome;
/// Add several scanned sessions (each already carrying a session key); returns one AddOutcome per input.
pub fn add_scanned(sessions: &[core::scan::ScannedSession]) -> Vec<AddOutcome>;
```
`add_from_session_key`: `core::api::fetch_account` + `parse_account` (None -> `Rejected("not accepted")` — Review Focus 2, writes nothing), fetch orgs, then `lock_store()` + `load_accounts_for_write()` + `core::key_intake::apply_session_key(..)` + `save_accounts()`, mapping `IntakeOutcome` -> `AddOutcome`. Runs on a worker thread (never the UI thread); the UI calls it off `invoke_from_event_loop`.

- [ ] **Step 1: failing tests** — these hit the network/store, so test the PURE mapping where possible and gate the rest. At minimum:
  - a unit test that `IntakeOutcome::{Added,Updated,RejectNoChatOrg}` map to the right `AddOutcome` variants (extract a pure `fn map_outcome(IntakeOutcome) -> AddOutcome` and test it).
  - `paste_rejects_unaccepted_key_without_writing` (Review Focus 2): using a loopback API base (reuse the helper-test loopback pattern, or point `CLAUDE_DASHBOARD_API_BASE` at a closed port) and a temp `APPDATA`, `add_from_session_key("sk-bad")` returns `Rejected(..)` and `accounts.json` is NOT created. (This mirrors the bridge roundtrip test's rejected path.)
- [ ] **Step 2-4:** run-fail, implement, run-pass.
- [ ] **Step 5:** `cargo test -p claude-dashboard` + build + clippy.
- [ ] **Step 6: commit** `feat(windows): add-account glue for pasted and scanned keys`.

---

### Task 5: Add Account wizard UI (3 tabs)

**Files:** `apps/windows/app/ui/` new `SetupWizard` component (its own .slint or added to app.slint); `apps/windows/app/src/setup.rs` (wire scan/paste/extension); main.rs (open the wizard from the "Add Account" affordances — the empty-state button from sub-project 2 and a Settings › Accounts "Add Account" button).

**Reference:** macOS `SetupView.swift` (tab structure, detected-account list with checkboxes, scan progress/errors).

**Interfaces — produces:** a modal/sheet wizard with three tabs:
1. **Browser extension (default):** lists installed Chromium browsers; a "How to install" section (link/instructions to load the unpacked extension at `apps/windows/extension` + run `register-dev-host.ps1`, or the store page when published); LIVE status that watches the account store / `extension_sources` bindings and shows "Waiting for a key…" -> "Added <email>" when the bridge writes one (poll the store every ~2s while the tab is open, or watch for a store mtime change).
2. **Scan browser:** a "Scan" button runs `core::scan::scan_windows_profiles()` off-thread (`invoke_from_event_loop` to deliver results); shows detected sessions as a checkbox list (name/email/plan once resolved); `AppBound` profiles show a "use the browser extension instead" note (Review Focus 1); "Add selected" calls `setup::add_scanned(selected)` and reports per-account outcomes.
3. **Paste a session key:** a text field + "Add"; calls `setup::add_from_session_key(trimmed)` off-thread; shows the outcome (Added/Updated/Rejected/No chat org).

- [ ] **Step 1:** build the wizard UI bound to Rust-side callbacks; the scan/paste/add run on a worker thread and post results via invoke_from_event_loop; after a successful add, nudge the refresh loop and close (or let the user add more).
- [ ] **Step 2:** wire the "Add Account" affordances (sub-project-2 empty-state button; the Accounts settings button in Task 6) to open the wizard.
- [ ] **Step 3: build & run** — `cargo build -p claude-dashboard` clean, clippy clean. Smoke (FAKE_ROWS + SMOKE) exits 0. Controller run-check: open the wizard, confirm the three tabs render and the paste tab accepts input (a real add needs a live key — verify the "Rejected" path with a bad key, which is deterministic).
- [ ] **Step 4: commit** `feat(windows): Add Account wizard (extension / scan / paste)`.

---

### Task 6: Settings › Accounts pane

**Files:** `apps/windows/app/src/settings_accounts.rs` (glue, TDD the delete+mute); `apps/windows/app/ui/` AccountsSettings pane; main.rs/app.slint (enable the Settings › Accounts sidebar item).

**Reference:** macOS `AccountsSettingsPane.swift` (list, delete, Re-sync All; no rename).

**Interfaces — produces (glue):**
```rust
/// Remove an account by id and, if an extension install was bound to it, mute that install
/// so the extension can't silently re-add it. Returns the muted installId if any.
pub fn delete_account(account_id: &str) -> Result<Option<String>, String>;
pub fn unmute(install_id: &str) -> Result<(), String>;
pub fn muted_sources() -> Vec<String>;   // installIds currently muted (for the "muted sources" list)
```
`delete_account`: under `lock_store()`, `load_accounts_for_write`, remove the row by id, `save_accounts`; then in `extension_sources` (same lock held), find the binding whose `account_id` matches and add its `installId` to `muted`, `save` sources; return that installId.

- [ ] **Step 1: failing test** `delete_mutes_the_extension_install` (Review Focus 3): seed a temp store with an account `ACC-1` and an `extension_sources` binding `inst-1 -> ACC-1`; `delete_account("ACC-1")` removes the account, returns `Some("inst-1")`, and `is_muted("inst-1")` is true afterwards; `unmute("inst-1")` clears it; deleting a non-extension account returns `Ok(None)`.
- [ ] **Step 2-4:** run-fail, implement, run-pass.
- [ ] **Step 5:** AccountsSettings pane UI: the account list (avatar/name/email/plan), a Delete button per row (confirm, then `delete_account` + nudge refresh), an "Add Account" button (opens the Task-5 wizard), "Re-sync All" (nudge the loop), and a "Muted sources" section listing `muted_sources()` with an Unmute button each. Enable the Settings › Accounts sidebar item.
- [ ] **Step 6: build & run** — build/clippy/`cargo test -p claude-dashboard` (incl the delete-mute test) green; smoke exits 0; controller run-check: the pane lists accounts and delete works (with fake rows the delete path can be exercised against a temp store).
- [ ] **Step 7: commit** `feat(windows): Settings Accounts pane (delete, mute, re-sync)`.

---

### Task 7: Settings › General pane + live Auto Refresh

**Files:** `apps/windows/app/src/settings_general.rs`; `apps/windows/app/ui/` GeneralSettings pane; `apps/windows/app/src/refresh.rs` + main.rs (loop reads the interval from `core::settings` live); enable the Settings › General sidebar item.

**Reference:** macOS `GeneralSettingsPane.swift` (About header, Auto Refresh, Updates).

**Interfaces — produces:** the General pane with:
- **About:** app name + version (read the crate version), a short line; Slint `AboutSlint` attribution per the Slint royalty-free license.
- **Auto Refresh:** a control (dropdown or stepper) bound to `core::settings.auto_refresh_seconds`; on change, `settings::save` and the running loop picks up the new interval on its next tick (see below). Use `effective_refresh_seconds()` for the actual timer (Review Focus 4: clamp/default).
- **Launch at startup:** a toggle bound to `core::startup::is_enabled()`; on change, `enable(exe)`/`disable()` and mirror into `settings.launch_at_startup`.
- **Updates:** a disabled "Check for updates" placeholder ("Coming soon" — auto-update is sub-project 6).

Loop change: the refresh loop computes its sleep each iteration from `core::settings::load().effective_refresh_seconds()` (so a settings change takes effect next cycle without restart), or waits on the nudge channel with that timeout. A manual Auto-Refresh change may also nudge for an immediate apply.

- [ ] **Step 1:** make the loop read the interval each iteration via `effective_refresh_seconds()`; add a test `refresh_interval_clamps_and_defaults` (Review Focus 4) exercising `Settings::effective_refresh_seconds` edge values if not already covered by Task 1 (reference it) — and a small test that the loop's chosen timeout equals `effective_refresh_seconds()` for a given Settings (extract the timeout calc into a pure fn and test it).
- [ ] **Step 2:** build the General pane UI + wire Auto Refresh (-> settings save + nudge), Launch at startup (-> core::startup), About + Slint attribution, Updates placeholder. Enable the Settings › General sidebar item.
- [ ] **Step 3: build & run** — build/clippy/tests green; smoke exits 0; controller run-check: the pane renders, toggling Launch at startup flips the Run key (verify via `core::startup::is_enabled()` or reg query), changing Auto Refresh persists to settings.json.
- [ ] **Step 4: commit** `feat(windows): Settings General pane (auto refresh, launch at startup)`.

---

### Task 8: CI, docs, contract

**Files:** `.github/workflows/ci.yml` (confirm core tests cover settings/scan/startup — the existing `cargo test -p claude-dashboard-core` does; no new job needed); `CLAUDE.md`; `contract/windows.md`.

- [ ] **Step 1:** CLAUDE.md — add `core::settings`, `core::scan`, `core::startup` to the Windows core modules list, and note the app's Setup wizard + Settings panes under `apps/windows/app`.
- [ ] **Step 2:** contract/windows.md — document: `settings.json` shape + location + that it's non-critical (missing/corrupt -> defaults); the Launch-at-startup Run key (`HKCU\...\Run`, value `ClaudeDashboard`, quoted exe path); the scan "app-bound -> use the extension" behaviour; and that deleting an extension-sourced account mutes its install (cross-reference the existing muted rule).
- [ ] **Step 3:** confirm CI green locally where possible: `cd apps/linux && cargo test -p claude-dashboard-core` (settings/scan/startup), `cd ../windows && cargo test -p claude-dashboard` and `cargo clippy --all-targets -- -D warnings`.
- [ ] **Step 4: commit** `docs(windows): settings/scan/startup + Setup & Settings docs`.

---

## Done when

- `core::settings`, `core::scan` (classify), and `core::startup` are implemented with passing tests; `cargo test -p claude-dashboard-core` green on Windows and (via CI) Linux.
- The paste/scan add-glue and the delete+mute glue have passing tests; `cargo test -p claude-dashboard` green.
- A controller/user run-check shows: the Add Account wizard (three tabs; paste-bad-key rejected; scan lists `v10` profiles and flags `v20` as needing the extension), the Accounts settings pane (list + delete + unmute), and the General pane (About, Auto Refresh persisting, Launch-at-startup toggling the Run key) — with screenshots.
- Deleting an extension-sourced account mutes its install; Auto Refresh changes take effect without restart.

Sub-project 4 (charts) consumes: the usage-log store (already written each refresh), `core::geometry`/`core::colors`, and the per-account page's disabled "View chart" button becomes real.
