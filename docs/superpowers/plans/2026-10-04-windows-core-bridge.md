# Windows Sub-project 1: Core on Windows + Bridge + Extension — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `apps/linux/core` build and pass its tests on Windows (paths, at-rest key protection, Windows cookie format), and deliver the key pipeline the Windows app relies on: a browser extension that reads the claude.ai `sessionKey` through the supported `chrome.cookies` API and a native-messaging host that turns it into a stored account.

**Architecture:** `core` gains `cfg(windows)` branches (store paths, at-rest encryption via the OS user-data protection API, Windows profile discovery, Local-State key unwrap) plus two platform-neutral additions: a cross-process store lock and `key_intake`, the add-or-repair logic lifted out of the Linux helper's `add-key` so the host binary can share it. A new Cargo workspace `apps/windows/` holds the native-messaging host, whose side effects go through an injected `Environment` trait so tests never touch the network, the store or the pipe. The extension is plain ES modules with its logic in `lib/`, tested under `node --test` with the browser APIs mocked.

**Tech Stack:** Rust 1.98.0 (pinned), `windows-sys` 0.59, `serde`/`serde_json`, `uuid`; Chrome MV3 extension (ES modules); Node ≥ 20 for `node --test`; PowerShell for the dev registration script; GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-04-windows-app-design.md` (sub-project 1 of 6).

**Prior art (this is a port, not new behaviour):** the macOS app (`apps/macos/`) and the Linux helper (`apps/linux/`) already do all of this on their platforms, against the user's own browser on the user's own machine. This sub-project reuses `apps/linux/core` and mirrors `apps/linux/helper/src/add_key.rs`. Every rule it follows is already fixed in `contract/` and exercised by existing tests.

## Global Constraints

- Toolchain: Rust `1.98.0` via `rust-toolchain.toml` (both workspaces); edition 2021.
- Windows 11 is the target OS; the OS-specific code is `cfg(windows)`; everything else in `core` must keep building and passing on Linux.
- Store locations on Windows: `%APPDATA%\claude-dashboard\accounts.json`, `%LOCALAPPDATA%\claude-dashboard\usage_logs.db`.
- Session keys at rest on Windows: the current-user OS protection API (`CryptProtectData`/`CryptUnprotectData`), base64-encoded, same wire shape (one string) as the Unix scheme but not byte-compatible.
- A Chrome-127+ app-bound cookie (tag `v20`) returns `CookieError::AppBoundEncrypted`; the code never attempts to open it and the UI points the user at the extension instead.
- Native-messaging host name: `com.claude_dashboard.bridge`.
- Message in: `{ "type": "sessionKey", "installId", "browser", "sessionKey" }`; reply `{ "ok": true, "email" }` or `{ "ok": false, "error": "<code>", "message" }`.
- Named pipe for "reload the app": `\\.\pipe\claude-dashboard`, message `reload\n`; a missing pipe is not an error.
- Store writers hold an exclusive lock on `accounts.json.lock`, then write a temp file and rename.
- The session key never appears in any log, stderr line, reply or error message.
- Account JSON schema (`contract/account-schema.md`) is unchanged: extension-sourced accounts are stored with `source: "manual"`; the install→account binding lives in a separate `extension-sources.json`.
- No new `core` dependency other than `windows-sys` (Windows-only target dependency).

## Review Focus

1. **A key arriving from the extension for an account that was pasted earlier** must repair that record (new key, status active), never add a duplicate. → Task 9, test `known_identity_is_repaired_not_duplicated`.
2. **A muted install keeps sending keys** (user deleted that account): nothing is written to either store, reply code is `muted`. → Task 9, test `muted_install_writes_nothing`.
3. **Malformed host input** — a length prefix above the cap, a stream ending mid-message, non-JSON bytes — produces an error reply or clean exit, never a panic or an unbounded allocation. → Task 7 tests `oversized_length_is_rejected`, `truncated_body_is_an_error`; Task 9 test `non_json_is_bad_message`.
4. **App and host saving at the same moment** must not lose either write. → Task 3, test `lock_store_serialises_writers`.
5. **The extension cannot reach the host** (app not installed / not registered) shows a badge and a readable status, never fails silently. → Task 11, test `unreachable_host_sets_badge_and_status`.

## File Structure

```
apps/linux/core/
  Cargo.toml                     + windows-sys (target cfg(windows))
  src/lib.rs                     + pub mod userprotect (cfg windows), pub mod key_intake
  src/store.rs                   Windows paths, cfg(unix) perms, protect branch, lock_store()
  src/userprotect.rs    (new)    protect/unprotect via the OS user-data API
  src/plan.rs                    + ParsedOrg, parse_orgs, plan_for, refreshed_plan_for, plan_wire_value (moved from helper)
  src/key_intake.rs     (new)    apply_session_key(): add-or-repair, shared by helper add-key and the host
  src/cookie/mod.rs              + CookieError::AppBoundEncrypted, gcm_open() shared with v12, pub mod win
  src/cookie/win.rs     (new)    decrypt_windows_cookie_value, local_state_encrypted_key, windows_cookie_key
  src/browser.rs                 + WindowsProfile, discover_windows_profiles_under()
apps/linux/helper/src/
  sync.rs                        plan helpers removed (imported from core::plan)
  add_key.rs                     uses core::key_intake
apps/windows/                    (new Cargo workspace)
  Cargo.toml, rust-toolchain.toml, .gitignore
  bridge/Cargo.toml
  bridge/src/main.rs             stdin -> handle -> stdout
  bridge/src/lib.rs
  bridge/src/framing.rs          4-byte LE length + JSON
  bridge/src/sources.rs          extension-sources.json (bindings, muted)
  bridge/src/handler.rs          Environment trait, handle(), Reply
  bridge/src/real_env.rs         RealEnv: network, store, pipe
  bridge/com.claude_dashboard.bridge.json   host manifest template
  scripts/register-dev-host.ps1  writes manifest + HKCU registry keys
  extension/manifest.json
  extension/background.js
  extension/popup.html, extension/popup.js
  extension/lib/sync.js          syncOnce(), detectBrowser(), getInstallId()
  extension/lib/status.js        describeResult()
  extension/lib/extension-id.js  extensionIdFromKey()
  extension/scripts/gen-key.mjs
  extension/tests/*.test.mjs
contract/windows.md     (new)
.github/workflows/ci.yml (new)
CLAUDE.md                        + Windows section and test commands
```

Each task below is added to this file in its own step. Tasks are dependency-ordered.

---

### Task 0: Windows toolchain

**Files:** none (machine setup). Ask the user before the installs — they are system-wide.

- [ ] **Step 1: Check what is installed**

Run (PowerShell): `rustup --version; cargo --version; node --version; Get-Command cl.exe -ErrorAction SilentlyContinue`
Expected today: rustup/cargo missing, node present.

- [ ] **Step 2: Install rustup and MSVC build tools (with approval)**

```powershell
winget install --id Rustlang.Rustup -e
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive --wait"
```

`rusqlite`'s `bundled` feature compiles SQLite from C, so the MSVC C compiler is required.

- [ ] **Step 3: Verify in a new shell**

Run: `cd apps/linux; cargo --version`
Expected: `cargo 1.98.0 ...` (rustup installs the pinned toolchain on first use).

- [ ] **Step 4: Record the baseline**

Run: `cd apps/linux; cargo test -p claude-dashboard-core`
Expected: compile error `could not find 'unix' in 'os'` from `core/src/store.rs:21`. Task 1 fixes it.

---

### Task 1: core builds on Windows with Windows store paths

**Files:**
- Modify: `apps/linux/core/src/store.rs` (imports ~17-31, paths 77-102, `save_accounts` 186-217, `write_and_publish` 221-247, tests module from ~540)
- Modify: `apps/linux/core/src/browser.rs` (the test calling `std::os::unix::fs::symlink`, near line 629)

**Interfaces:**
- Produces: `store::accounts_path()` and `store::usage_log_path()` resolve under `%APPDATA%`/`%LOCALAPPDATA%` on Windows, XDG on Unix (unchanged). Test helper `point_store_at(dir: &Path)` in `store::tests`.

- [ ] **Step 1: Write the failing test** — append to `mod tests` in `store.rs`:

```rust
    /// Points every store path at `dir`, whichever platform's variables the
    /// path functions read. Callers hold `env_lock()`.
    fn point_store_at(dir: &Path) {
        for var in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "APPDATA", "LOCALAPPDATA"] {
            env::set_var(var, dir);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_store_lives_under_appdata_and_localappdata() {
        let _guard = env_lock();
        env::set_var("APPDATA", r"C:\Users\u\AppData\Roaming");
        env::set_var("LOCALAPPDATA", r"C:\Users\u\AppData\Local");
        assert_eq!(
            accounts_path(),
            PathBuf::from(r"C:\Users\u\AppData\Roaming\claude-dashboard\accounts.json")
        );
        assert_eq!(
            usage_log_path(),
            PathBuf::from(r"C:\Users\u\AppData\Local\claude-dashboard\usage_logs.db")
        );
    }
```

Then replace every `env::set_var("XDG_CONFIG_HOME", dir.path());` (and the `std::env::` form) in the tests module with `point_store_at(dir.path());`.

- [ ] **Step 2: Run to confirm it fails**

Run: `cd apps/linux; cargo test -p claude-dashboard-core windows_store`
Expected: FAIL — compile error on `std::os::unix`.

- [ ] **Step 3: Implement** — gate the Unix import:

```rust
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
```

Replace `home_dir`, `xdg_dir`, `accounts_path`, `usage_log_path` with platform-split directory helpers plus shared path builders:

```rust
#[cfg(unix)]
fn home_dir() -> PathBuf {
    env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(unix)]
fn xdg_dir(var: &str, fallback: &[&str]) -> PathBuf {
    match env::var(var) {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let mut p = home_dir();
            for part in fallback {
                p.push(part);
            }
            p
        }
    }
}

#[cfg(unix)]
fn config_dir() -> PathBuf { xdg_dir("XDG_CONFIG_HOME", &[".config"]) }
#[cfg(unix)]
fn data_dir() -> PathBuf { xdg_dir("XDG_DATA_HOME", &[".local", "share"]) }

#[cfg(windows)]
fn config_dir() -> PathBuf { known_dir("APPDATA") }
#[cfg(windows)]
fn data_dir() -> PathBuf { known_dir("LOCALAPPDATA") }

/// Windows always sets both for an interactive user; `.` only keeps a broken
/// environment from panicking.
#[cfg(windows)]
fn known_dir(var: &str) -> PathBuf {
    match env::var(var) {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from("."),
    }
}

pub fn accounts_path() -> PathBuf {
    config_dir().join("claude-dashboard").join("accounts.json")
}
pub fn usage_log_path() -> PathBuf {
    data_dir().join("claude-dashboard").join("usage_logs.db")
}
```

In `save_accounts`, gate the permission line with `#[cfg(unix)]` (on Windows `%APPDATA%` is already per-user by ACL; add a one-line comment saying so).

In `write_and_publish`, build the open options cross-platform and close the handle before the rename (Windows will not reliably rename a file this process still holds open):

```rust
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(tmp)?;
    #[cfg(test)]
    { fail_if_fault_injected("after_open")?; }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    #[cfg(test)]
    { fail_if_fault_injected("after_write")?; }
    fs::rename(tmp, path)?;
    Ok(())
```

(`std::fs::rename` on Windows is `MoveFileExW` with replace-existing, so it still swaps the destination.)

- [ ] **Step 4: Gate Unix-only tests**

Add `#[cfg(unix)]` above each test that asserts a Unix filesystem property: in `store.rs` `save_accounts_writes_the_store_mode_600`, `save_accounts_creates_the_leaf_dir_mode_700`, `save_accounts_repairs_a_pre_existing_644_store`, `an_unreadable_store_is_an_error_and_is_not_quarantined` (mode-000); in `browser.rs` the symlink test. Each gate gets a one-line comment naming the property it needs.

- [ ] **Step 5: Run the whole crate on Windows**

Run: `cd apps/linux; cargo test -p claude-dashboard-core`
Expected: PASS incl. the new test. A remaining failure that is only a Unix-filesystem assumption gets `#[cfg(unix)]`; any other failure is a real bug — fix it, do not gate it.

- [ ] **Step 6: Clippy, then commit**

```bash
cd apps/linux && cargo clippy -p claude-dashboard-core --all-targets -- -D warnings
git add apps/linux/core/src/store.rs apps/linux/core/src/browser.rs
git commit -m "feat(core): build on Windows with APPDATA/LOCALAPPDATA store paths"
```

---

### Task 2: at-rest session-key protection on Windows

**Files:**
- Modify: `apps/linux/core/Cargo.toml`
- Create: `apps/linux/core/src/userprotect.rs`
- Modify: `apps/linux/core/src/lib.rs`
- Modify: `apps/linux/core/src/store.rs` (encryption section, ~249-332)

**Interfaces:**
- Produces: `core::userprotect::protect(&[u8]) -> Option<Vec<u8>>`, `core::userprotect::unprotect(&[u8]) -> Option<Vec<u8>>` (Windows only). `store::encrypt_session_key(&str) -> String` and `store::decrypt_session_key(&str) -> Option<String>` keep their signatures on both platforms.

This is the direct Windows counterpart of the existing Unix at-rest scheme in `store.rs` (HKDF over the machine id). It wraps the standard per-user data-protection API the OS provides; it does not touch any browser data.

- [ ] **Step 1: Add the dependency** — `apps/linux/core/Cargo.toml`:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = ["Win32_Foundation", "Win32_Security_Cryptography"] }
```

- [ ] **Step 2: Write the failing tests** — create `apps/linux/core/src/userprotect.rs` with tests only:

```rust
//! Per-user data protection for values stored at rest (CryptProtectData).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protect_then_unprotect_round_trips() {
        let sealed = protect(b"sk-ant-sid01-example").expect("protect");
        assert_ne!(sealed.as_slice(), b"sk-ant-sid01-example");
        assert_eq!(unprotect(&sealed).expect("unprotect"), b"sk-ant-sid01-example");
    }

    #[test]
    fn unprotect_rejects_garbage() {
        assert_eq!(unprotect(b"not a protected blob"), None);
    }
}
```

Register in `lib.rs`:

```rust
#[cfg(windows)]
pub mod userprotect;
```

- [ ] **Step 3: Run to confirm it fails**

Run: `cd apps/linux; cargo test -p claude-dashboard-core userprotect`
Expected: FAIL — `cannot find function 'protect'`.

- [ ] **Step 4: Implement** — above the tests. Call the OS API with a null description, null entropy, null prompt, and the UI-forbidden flag; copy the result out and free it with `LocalFree`:

```rust
use std::ptr;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

fn input_blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
}

/// Copies the API's output out and frees it with `LocalFree`.
/// # Safety
/// `out` must be a blob the API just filled on success.
unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    LocalFree(out.pbData as _);
    bytes
}

/// Seals `plain` to the current user. `None` only if the API fails.
pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    let input = input_blob(plain);
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
    // SAFETY: every pointer is valid for the call; `out` is freed by `take`.
    let ok = unsafe {
        CryptProtectData(&input, ptr::null(), ptr::null(), ptr::null(),
                         ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
    };
    if ok == 0 { return None; }
    Some(unsafe { take(out) })
}

/// Opens a blob sealed by [`protect`] (or by the browser for its Local State).
/// `None` for anything the API rejects: another user's blob, garbage, truncation.
pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    let input = input_blob(sealed);
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
    // SAFETY: as in `protect`.
    let ok = unsafe {
        CryptUnprotectData(&input, ptr::null_mut(), ptr::null(), ptr::null(),
                           ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
    };
    if ok == 0 { return None; }
    Some(unsafe { take(out) })
}
```

If the compiler reports an argument-type mismatch for `windows-sys` 0.59 (e.g. the description out-param of `CryptUnprotectData`), follow the compiler's expected type; the call shape is unchanged.

- [ ] **Step 5: Switch the store's at-rest scheme on Windows** — in `store.rs`, gate the existing Unix scheme (`HKDF_SALT`, `machine_id_bytes`, `derive_key`, `encrypt_session_key`, `decrypt_session_key`, and the `aes_gcm`/`hkdf`/`Hkdf`/`Sha256`/`Key`/`Nonce` imports) with `#[cfg(unix)]`, and add the Windows pair, keeping the same base64 wire shape:

```rust
#[cfg(windows)]
pub fn encrypt_session_key(plain: &str) -> String {
    let sealed = crate::userprotect::protect(plain.as_bytes())
        .expect("CryptProtectData does not fail for the logged-in user");
    BASE64.encode(sealed)
}

#[cfg(windows)]
pub fn decrypt_session_key(cipher_b64: &str) -> Option<String> {
    let sealed = BASE64.decode(cipher_b64).ok()?;
    String::from_utf8(crate::userprotect::unprotect(&sealed)?).ok()
}
```

Keep the `BASE64`/`Engine` imports ungated (both schemes use them).

- [ ] **Step 6: Run**

Run: `cd apps/linux; cargo test -p claude-dashboard-core`
Expected: PASS — `userprotect::tests::*` plus the existing `session_key_encrypt_decrypt_roundtrip` and `session_key_decrypt_rejects_garbage`, now going through the Windows path.

- [ ] **Step 7: Clippy, then commit**

```bash
cd apps/linux && cargo clippy -p claude-dashboard-core --all-targets -- -D warnings
git add apps/linux/core/Cargo.toml apps/linux/Cargo.lock apps/linux/core/src/userprotect.rs apps/linux/core/src/lib.rs apps/linux/core/src/store.rs
git commit -m "feat(core): per-user at-rest session-key protection on Windows"
```

---

### Task 3: cross-process store lock

**Files:**
- Modify: `apps/linux/core/src/store.rs`

**Interfaces:**
- Produces: `pub struct StoreLock` (releases on drop) and `pub fn lock_store() -> Result<StoreLock, StoreError>`. Callers hold the guard across `load_accounts_for_write` -> modify -> `save_accounts`. Used by the host (Task 9) and later the app.

- [ ] **Step 1: Write the failing test** — in `store::tests`:

```rust
    #[test]
    fn lock_store_serialises_writers() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        point_store_at(dir.path());

        let first = lock_store().unwrap();
        let released = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&released);
        let second = std::thread::spawn(move || {
            let _lock = lock_store().unwrap();
            seen.load(Ordering::SeqCst)
        });

        std::thread::sleep(Duration::from_millis(200));
        released.store(true, Ordering::SeqCst);
        drop(first);

        assert!(second.join().unwrap(), "second lock granted while first was held");
        assert!(dir.path().join("claude-dashboard").join("accounts.json.lock").is_file());
    }
```

- [ ] **Step 2: Run to confirm it fails**

Run: `cd apps/linux; cargo test -p claude-dashboard-core lock_store`
Expected: FAIL — `cannot find function 'lock_store'`.

- [ ] **Step 3: Implement** — after `write_and_publish`:

```rust
/// Held for the whole read-modify-write of `accounts.json`. Two processes
/// (the Windows app and its native-messaging host) write the store; without
/// this, each can load, change and save, and the second save drops the
/// first's change. Released when dropped.
pub struct StoreLock {
    _file: fs::File,
}

/// Blocks until this process holds the exclusive lock on
/// `accounts.json.lock`, a sibling of the store (separate file, because the
/// store itself is replaced by rename on every save).
pub fn lock_store() -> Result<StoreLock, StoreError> {
    let path = accounts_path().with_extension("json.lock");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::OpenOptions::new()
        .read(true).write(true).create(true).truncate(false)
        .open(&path)?;
    file.lock()?;
    Ok(StoreLock { _file: file })
}
```

(`File::lock` is stable since Rust 1.89: `flock` on Unix, `LockFileEx` on Windows.)

- [ ] **Step 4: Run**

Run: `cd apps/linux; cargo test -p claude-dashboard-core lock_store`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/linux/core/src/store.rs
git commit -m "feat(core): cross-process lock for store read-modify-write"
```

---

### Task 4: key_intake — add-or-repair shared by add-key and the host

**Files:**
- Modify: `apps/linux/core/src/plan.rs` (receives `ParsedOrg`, `parse_orgs`, `plan_for`, `refreshed_plan_for`, `plan_wire_value`)
- Create: `apps/linux/core/src/key_intake.rs`
- Modify: `apps/linux/core/src/lib.rs`
- Modify: `apps/linux/helper/src/sync.rs` (delete the moved items ~395-500, import them instead)
- Modify: `apps/linux/helper/src/add_key.rs`

**Interfaces:**
- Produces (in `core::plan`): `pub struct ParsedOrg { pub uuid: String, pub capabilities: Vec<String>, pub raw: serde_json::Value }`, `pub fn parse_orgs(&str) -> Vec<ParsedOrg>`, `pub fn plan_for(&[ParsedOrg], &str) -> AccountPlan`, `pub fn refreshed_plan_for(&Account, &[ParsedOrg]) -> Option<AccountPlan>`, `pub fn plan_wire_value(&AccountPlan) -> String`.
- Produces (in `core::key_intake`):
  ```rust
  pub enum IntakeOutcome {
      Added { account_id: String, name: String, plan: AccountPlan },
      Updated { account_id: String, name: String, old_plan: AccountPlan, new_plan: AccountPlan, warn_no_chat_org: bool },
      RejectNoChatOrg,
  }
  pub fn apply_session_key(
      accounts: &mut Vec<Account>,
      session_key: &str,
      identity: &ParsedAccount,
      orgs: &[ParsedOrg],
      new_id: impl FnOnce() -> String,
      now_reference: f64,
  ) -> IntakeOutcome
  ```
  It mutates `accounts` but never loads, saves or fetches.

This is a pure refactor plus move: the add/repair body currently in `add_key.rs` moves into `core` so the host (Task 9) runs the identical logic. The rules are already pinned by `contract/cases/dedupe.json` and `contract/cases/manual-key.json`.

- [ ] **Step 1: Move the plan helpers** — cut `ParsedOrg`, `parse_orgs`, `plan_for`, `refreshed_plan_for`, `plan_wire_value` (with doc comments) from `helper/src/sync.rs` into `core/src/plan.rs`, changing `pub(crate)` to `pub` on the struct, its fields and the functions. `plan.rs` needs `use crate::model::Account;`. In `sync.rs` add:

```rust
use claude_dashboard_core::plan::{
    parse_orgs, plan_for, plan_wire_value, refreshed_plan_for, ParsedOrg,
};
```

and drop now-unused imports (`detect_plan_tier`, possibly `serde_json::Value`). In `add_key.rs` change `use crate::sync::{...}` to import from `core::plan`.

Run: `cd apps/linux; cargo build --workspace`
Expected: builds (no behaviour change yet).

- [ ] **Step 2: Write the failing tests** — create `core/src/key_intake.rs` with the module doc and tests only. Cover: a new identity added as manual+active with the key stored and decryptable; an identity without email named from its uuid; no-chat-org rejected writing nothing; a known identity repaired in place (no new id minted); a legacy email-matched record getting its uuid backfilled.

```rust
//! What a session key does to the account list: add a new account or repair
//! the stored one. Shared by the Linux helper's add-key and the Windows
//! native-messaging host, so both follow contract/cases/dedupe.json and
//! contract/cases/manual-key.json through one implementation.
//!
//! No I/O: the caller fetches /api/account and /api/organizations, holds the
//! store lock, loads, calls this, and saves.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::parse_account;
    use crate::plan::parse_orgs;
    use crate::store::decrypt_session_key;

    fn identity(uuid: &str, email: Option<&str>, caps: &str) -> ParsedAccount {
        let email_field = email.map(|e| format!(r#""email_address":"{e}","#)).unwrap_or_default();
        parse_account(&format!(
            r#"{{{email_field}"uuid":"{uuid}","memberships":[
                {{"organization":{{"uuid":"org-1","name":"Org","capabilities":{caps}}}}}]}}"#
        ))
        .unwrap()
    }

    fn orgs(caps: &str) -> Vec<ParsedOrg> {
        parse_orgs(&format!(r#"[{{"uuid":"org-1","name":"Org","capabilities":{caps}}}]"#))
    }

    fn stored(uuid: Option<&str>, email: &str) -> Account {
        let uuid_field = uuid.map(|u| format!(r#""accountUuid":"{u}","#)).unwrap_or_default();
        Account::from_json_object(&format!(
            r#"{{"id":"OLD-ID","name":"{email}","email":"{email}",{uuid_field}
                "chromeProfilePath":"","orgId":"org-1","plan":"Pro","status":"expired",
                "source":"manual"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn a_new_identity_is_added_as_a_manual_active_account() {
        let mut accounts = Vec::new();
        let outcome = apply_session_key(
            &mut accounts, "sk-new",
            &identity("acct-1", Some("a@x.com"), r#"["chat","claude_max"]"#),
            &orgs(r#"["chat","claude_max"]"#),
            || "NEW-ID".to_string(), 42.0,
        );
        assert_eq!(outcome, IntakeOutcome::Added {
            account_id: "NEW-ID".into(), name: "a@x.com".into(), plan: AccountPlan::Max200,
        });
        let a = &accounts[0];
        assert_eq!(a.account_uuid.as_deref(), Some("acct-1"));
        assert_eq!(a.org_id.as_deref(), Some("org-1"));
        assert_eq!(a.source, AccountSource::Manual);
        assert_eq!(a.status, AccountStatus::Active);
        assert_eq!(a.last_synced, Some(42.0));
        assert_eq!(decrypt_session_key(a.session_key.as_deref().unwrap()).as_deref(), Some("sk-new"));
    }

    #[test]
    fn an_identity_without_email_is_named_from_its_uuid() {
        let mut accounts = Vec::new();
        let outcome = apply_session_key(
            &mut accounts, "sk",
            &identity("abcdefghijk", None, r#"["chat"]"#),
            &[], || "ID".to_string(), 0.0,
        );
        assert!(matches!(outcome, IntakeOutcome::Added { ref name, .. } if name == "Account abcdefgh"));
        assert_eq!(accounts[0].plan, AccountPlan::Pro); // no orgs fetched -> add path falls back to Pro
    }

    #[test]
    fn no_chat_org_rejects_and_writes_nothing() {
        let mut accounts = Vec::new();
        let outcome = apply_session_key(
            &mut accounts, "sk",
            &identity("acct-1", Some("a@x.com"), r#"["api"]"#),
            &[], || "ID".to_string(), 0.0,
        );
        assert_eq!(outcome, IntakeOutcome::RejectNoChatOrg);
        assert!(accounts.is_empty());
    }

    #[test]
    fn a_known_identity_is_repaired_in_place() {
        let mut accounts = vec![stored(Some("acct-1"), "a@x.com")];
        let outcome = apply_session_key(
            &mut accounts, "sk-fresh",
            &identity("acct-1", Some("a@x.com"), r#"["chat","claude_max"]"#),
            &orgs(r#"["chat","claude_max"]"#),
            || panic!("repair must not mint an id"), 7.0,
        );
        assert_eq!(outcome, IntakeOutcome::Updated {
            account_id: "OLD-ID".into(), name: "a@x.com".into(),
            old_plan: AccountPlan::Pro, new_plan: AccountPlan::Max200, warn_no_chat_org: false,
        });
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].status, AccountStatus::Active);
        assert_eq!(accounts[0].last_synced, Some(7.0));
        assert_eq!(decrypt_session_key(accounts[0].session_key.as_deref().unwrap()).as_deref(), Some("sk-fresh"));
    }

    #[test]
    fn a_legacy_record_matched_by_email_gets_its_uuid_backfilled() {
        let mut accounts = vec![stored(None, "A@X.com")];
        apply_session_key(
            &mut accounts, "sk",
            &identity("acct-9", Some("a@x.com"), r#"["chat"]"#),
            &[], || panic!("repair must not mint an id"), 0.0,
        );
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_uuid.as_deref(), Some("acct-9"));
    }
}
```

Register in `lib.rs`: `pub mod key_intake;`

- [ ] **Step 3: Run to confirm it fails**

Run: `cd apps/linux; cargo test -p claude-dashboard-core key_intake`
Expected: FAIL — `cannot find function 'apply_session_key'`.

- [ ] **Step 4: Implement** — above the tests. Lift the body currently in `add_key.rs` (the `duplicate_index` / `StoredManualTarget` / `manual_key_decision` block and its Add and Repair arms) into `apply_session_key`, returning an `IntakeOutcome` instead of printing. Imports:

```rust
use crate::api::ParsedAccount;
use crate::identity::{duplicate_index, StoredIdentity};
use crate::manual_key::{manual_key_decision, ManualKeyDecision, StoredManualTarget};
use crate::model::{Account, AccountPlan, AccountSource, AccountStatus, Browser};
use crate::plan::{plan_for, refreshed_plan_for, ParsedOrg};
use crate::store::encrypt_session_key;
```

The `Add` arm builds the `Account` exactly as `add_key.rs` does today (id from `new_id()`, `source: AccountSource::Manual`, `status: Active`, `last_synced: Some(now_reference)`, key via `encrypt_session_key`) and returns `IntakeOutcome::Added`. The `Repair` arm mutates `accounts[index]` exactly as today (session key, status, last_synced, the three `writes` fields, then `refreshed_plan_for`) and returns `IntakeOutcome::Updated` carrying `old_plan`, the new plan and `warn_no_chat_org`. `RejectNoChatOrg` maps straight through.

- [ ] **Step 5: Run**

Run: `cd apps/linux; cargo test -p claude-dashboard-core key_intake`
Expected: PASS (5 tests).

- [ ] **Step 6: Rewire add_key.rs onto key_intake** — replace everything from `let stored: Vec<StoredIdentity> = ...` to the end of `run_add_key` with: fetch orgs (empty on failure), call `apply_session_key(&mut accounts, session_key, &identity, &orgs, || Uuid::new_v4().to_string().to_uppercase(), now_reference_seconds())`, then `match` the outcome, saving the store and printing the SAME stderr lines as today for each arm:
  - `RejectNoChatOrg` -> `"No organization with chat access."`, exit 1.
  - `Added { name, plan, .. }` -> save; on save error print `"Could not write the account store."` exit 1; else `"Added: {name} ({plan_wire_value})"` exit 0.
  - `Updated { name, old_plan, new_plan, warn_no_chat_org, .. }` -> save; then `"Updated key: {name}"`; if `new_plan != old_plan` the `"Updated plan: ..."` line; if `warn_no_chat_org` the warning line; exit 0.

Imports become `use claude_dashboard_core::key_intake::{apply_session_key, IntakeOutcome};` and `use claude_dashboard_core::plan::{parse_orgs, plan_wire_value};` plus the existing `fetch_account`, `fetch_organizations`, `parse_account`, `trimmed_key`, `store`, `Uuid`.

- [ ] **Step 7: Verify the helper still honours its contract**

Run: `cd apps/linux; cargo build --workspace; cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean. The helper integration tests drive the store through `XDG_CONFIG_HOME`, so they run on Linux/macOS (the `linux` CI job in Task 12). If a Linux or macOS machine is at hand, run `cd apps/linux && cargo test --workspace` there now. Expected: PASS, byte-identical stderr.

- [ ] **Step 8: Commit**

```bash
git add apps/linux/core/src/plan.rs apps/linux/core/src/key_intake.rs apps/linux/core/src/lib.rs apps/linux/helper/src/sync.rs apps/linux/helper/src/add_key.rs
git commit -m "refactor(core): lift add-key's add-or-repair into core::key_intake"
```

---

### Task 5: Windows cookie value decoding (reuse the existing GCM path; refuse app-bound)

**Context:** `core/src/cookie/mod.rs` already decodes the Linux cookie formats, including an AES-256-GCM path for the `v12` case. Windows cookies written before Chrome 127 use the same GCM layout under a `v10`/`v11` tag, with the key stored in the profile's `Local State`. This task reuses that existing GCM routine for Windows and, for the Chrome-127+ app-bound tag (`v20`), returns a typed "not supported here" so the UI routes the user to the extension instead. No new cryptography is introduced.

**Files:**
- Modify: `apps/linux/core/src/cookie/mod.rs`
- Create: `apps/linux/core/src/cookie/win.rs`

**Interfaces:**
- Produces: `CookieError::AppBoundEncrypted`; `cookie::win::decode_value(encrypted: &[u8], host_key: &str, db_schema_version: i64, key: &[u8; 32]) -> Result<String, CookieError>`; `cookie::win::local_state_wrapped_key(local_state_json: &str) -> Option<Vec<u8>>` (the sealed blob, `"DPAPI"` prefix stripped); `#[cfg(windows)] cookie::win::profile_key(local_state: &Path) -> Option<[u8; 32]>`.

- [ ] **Step 1: Share the GCM routine** — in `cookie/mod.rs`, factor the AES-256-GCM-plus-domain-hash-strip body out of `decrypt_v12` into a crate-visible helper so both Linux `v12` and Windows reuse one implementation:

```rust
/// AES-256-GCM over `nonce(12) || ciphertext || tag`, then the schema >= 24
/// prefix strip. Shared by Linux v12 and the Windows reader.
pub(crate) fn gcm_open(
    key: &[u8; 32], body: &[u8], host_key: &str, db_schema_version: i64,
) -> Result<String, CookieError> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    if body.len() < 12 + 16 {
        return Err(CookieError::DecryptFailed);
    }
    let mut pt = Aes256Gcm::new(key.into())
        .decrypt(Nonce::from_slice(&body[..12]), &body[12..])
        .map_err(|_| CookieError::DecryptFailed)?;
    strip_domain_hash(&mut pt, host_key, db_schema_version);
    String::from_utf8(pt).map_err(|_| CookieError::BadUtf8)
}
```

`decrypt_v12` then derives its key via HKDF as today and calls `gcm_open(&key, body, host_key, db_schema_version)`. Add the error variant and `pub mod win;`:

```rust
    /// A Chrome-127+ app-bound cookie: deliberately not opened here.
    AppBoundEncrypted,
```

- [ ] **Step 2: Write the failing tests** — create `cookie/win.rs`. Build fixtures with the same GCM layout the reader expects (tag || 12-byte nonce || ciphertext+tag, plaintext prefixed by `SHA256(host)` for schema >= 24), and assert: a `v10` value decodes and the domain hash is stripped; a `v20` value returns `AppBoundEncrypted` (not garbage); a wrong key fails authentication; too-short input and an unknown tag fail cleanly; `local_state_wrapped_key` reads `os_crypt.encrypted_key`, base64-decodes, and strips the `"DPAPI"` prefix, returning `None` when any part is missing or the prefix is absent.

```rust
//! Chromium cookie values on Windows.
//!
//! - v10/v11: AES-256-GCM (same layout core already decodes for Linux v12);
//!   the key is Local State's os_crypt.encrypted_key: base64, "DPAPI" prefix,
//!   then a blob sealed to the current user.
//! - v20: app-bound (Chrome 127+). Not opened here; reported as
//!   CookieError::AppBoundEncrypted so the app points the user at the extension.
//! - schema >= 24 domain-hash prefix: stripped exactly as on Linux.

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine as _;
    use sha2::{Digest, Sha256};

    const KEY: [u8; 32] = [7u8; 32];

    fn make(tag: &[u8], host: &str, secret: &str) -> Vec<u8> {
        let mut pt = Sha256::digest(host.as_bytes()).to_vec();
        pt.extend_from_slice(secret.as_bytes());
        let nonce = [9u8; 12];
        let ct = Aes256Gcm::new(&KEY.into())
            .encrypt(Nonce::from_slice(&nonce), pt.as_slice()).unwrap();
        [tag, &nonce, &ct].concat()
    }

    #[test]
    fn v10_decodes_and_strips_the_domain_hash() {
        let blob = make(b"v10", "claude.ai", "sk-ant-sid01-WIN");
        assert_eq!(decode_value(&blob, "claude.ai", 24, &KEY).unwrap(), "sk-ant-sid01-WIN");
    }

    #[test]
    fn v20_is_reported_as_app_bound_not_garbage() {
        let blob = make(b"v20", "claude.ai", "sk-ant-sid01-WIN");
        assert_eq!(decode_value(&blob, "claude.ai", 24, &KEY), Err(CookieError::AppBoundEncrypted));
    }

    #[test]
    fn wrong_key_fails_authentication() {
        let blob = make(b"v10", "claude.ai", "sk");
        assert_eq!(decode_value(&blob, "claude.ai", 24, &[8u8; 32]), Err(CookieError::DecryptFailed));
    }

    #[test]
    fn short_and_unknown_tags_fail_cleanly() {
        assert_eq!(decode_value(b"v1", "claude.ai", 24, &KEY), Err(CookieError::TooShort));
        assert_eq!(decode_value(b"\x01\x00\x00raw", "claude.ai", 24, &KEY), Err(CookieError::DecryptFailed));
    }

    #[test]
    fn local_state_key_is_base64_minus_the_prefix() {
        let wrapped = [b"DPAPI".as_slice(), &[1, 2, 3]].concat();
        let json = format!(r#"{{"os_crypt":{{"encrypted_key":"{}"}}}}"#, BASE64.encode(&wrapped));
        assert_eq!(local_state_wrapped_key(&json), Some(vec![1, 2, 3]));
    }

    #[test]
    fn local_state_without_a_wrapped_key_is_none() {
        assert_eq!(local_state_wrapped_key("{}"), None);
        assert_eq!(local_state_wrapped_key("not json"), None);
        let no_prefix = format!(r#"{{"os_crypt":{{"encrypted_key":"{}"}}}}"#, BASE64.encode(b"XXXXXyz"));
        assert_eq!(local_state_wrapped_key(&no_prefix), None);
    }
}
```

- [ ] **Step 3: Run to confirm it fails**

Run: `cd apps/linux; cargo test -p claude-dashboard-core cookie::win`
Expected: FAIL — unresolved `decode_value`.

- [ ] **Step 4: Implement** — above the tests:

```rust
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::Value;

use super::{gcm_open, CookieError};

/// Decodes one cookie's value with the profile's unwrapped Local State key.
pub fn decode_value(
    encrypted: &[u8], host_key: &str, db_schema_version: i64, key: &[u8; 32],
) -> Result<String, CookieError> {
    if encrypted.len() < 3 {
        return Err(CookieError::TooShort);
    }
    match &encrypted[..3] {
        b"v20" => Err(CookieError::AppBoundEncrypted),
        b"v10" | b"v11" => gcm_open(key, &encrypted[3..], host_key, db_schema_version),
        _ => Err(CookieError::DecryptFailed),
    }
}

/// The sealed AES key from a Local State document: os_crypt.encrypted_key,
/// base64-decoded, with its "DPAPI" prefix removed. `None` if any step is missing.
pub fn local_state_wrapped_key(local_state_json: &str) -> Option<Vec<u8>> {
    let v: Value = serde_json::from_str(local_state_json).ok()?;
    let b64 = v.get("os_crypt")?.get("encrypted_key")?.as_str()?;
    let raw = BASE64.decode(b64).ok()?;
    raw.strip_prefix(b"DPAPI").map(<[u8]>::to_vec)
}

/// Reads `local_state`, unwraps its key with the per-user API, returns 32 bytes.
#[cfg(windows)]
pub fn profile_key(local_state: &std::path::Path) -> Option<[u8; 32]> {
    let json = std::fs::read_to_string(local_state).ok()?;
    let sealed = local_state_wrapped_key(&json)?;
    crate::userprotect::unprotect(&sealed)?.try_into().ok()
}
```

- [ ] **Step 5: Run the whole cookie module (Linux behaviour must not move)**

Run: `cd apps/linux; cargo test -p claude-dashboard-core cookie`
Expected: PASS — the new tests plus every existing Linux cookie test, including the real-blob known-answer test.

- [ ] **Step 6: Commit**

```bash
git add apps/linux/core/src/cookie/mod.rs apps/linux/core/src/cookie/win.rs
git commit -m "feat(core): decode Windows v10 cookie values, route v20 to the extension"
```

---

### Task 6: Windows browser-profile discovery

**Files:**
- Modify: `apps/linux/core/src/browser.rs`

**Interfaces:**
- Consumes: private `read_info_cache` in the same module.
- Produces:
  ```rust
  pub struct WindowsProfile {
      pub browser: Browser,
      pub profile_dir: String,
      pub display_name: Option<String>,
      pub google_email: Option<String>,
      pub cookies_db: PathBuf,
      pub local_state: PathBuf,
  }
  pub fn discover_windows_profiles_under(local_app_data: &Path) -> Vec<WindowsProfile>
  ```
  Pure path logic (no `cfg(windows)`), so it is tested on every OS. Sub-project 3 pairs it with `read_claude_cookie_db` + `profile_key` + `decode_value`.

- [ ] **Step 1: Write the failing test** — in `browser.rs`'s test module, create a temp `%LOCALAPPDATA%` with a Chrome profile whose cookie DB is at `Default\Network\Cookies` and an Edge profile at `Default\Cookies`, plus a Chrome `Local State` carrying one `info_cache` entry, and a Chrome profile with no cookie DB (must be skipped). Assert both profiles are found, in browser order, with the right `cookies_db`, `local_state`, `display_name` and `google_email`.

```rust
    #[test]
    fn windows_profiles_are_found_under_each_browsers_user_data() {
        let d = tempfile::tempdir().unwrap();
        let chrome = d.path().join("Google").join("Chrome").join("User Data");
        let edge = d.path().join("Microsoft").join("Edge").join("User Data");
        std::fs::create_dir_all(chrome.join("Default").join("Network")).unwrap();
        std::fs::write(chrome.join("Default").join("Network").join("Cookies"), b"").unwrap();
        std::fs::create_dir_all(chrome.join("Profile 2")).unwrap(); // no cookies: skipped
        std::fs::create_dir_all(edge.join("Default")).unwrap();
        std::fs::write(edge.join("Default").join("Cookies"), b"").unwrap();
        std::fs::write(
            chrome.join("Local State"),
            r#"{"profile":{"info_cache":{"Default":{"name":"Work","user_name":"me@x.com"}}}}"#,
        ).unwrap();

        let found = discover_windows_profiles_under(d.path());

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].browser, Browser::Chrome);
        assert_eq!(found[0].profile_dir, "Default");
        assert_eq!(found[0].display_name.as_deref(), Some("Work"));
        assert_eq!(found[0].google_email.as_deref(), Some("me@x.com"));
        assert_eq!(found[0].cookies_db, chrome.join("Default").join("Network").join("Cookies"));
        assert_eq!(found[0].local_state, chrome.join("Local State"));
        assert_eq!(found[1].browser, Browser::Edge);
        assert_eq!(found[1].cookies_db, edge.join("Default").join("Cookies"));
    }
```

Before relying on the fixture, confirm `read_info_cache`'s expected JSON shape and match it (the Linux tests in this module already have a working `Local State` fixture — copy its structure).

- [ ] **Step 2: Run to confirm it fails**

Run: `cd apps/linux; cargo test -p claude-dashboard-core windows_profiles`
Expected: FAIL — unresolved `discover_windows_profiles_under`.

- [ ] **Step 3: Implement** — after `push_profiles_in`. Iterate a fixed list of `(Browser, [parts])` user-data roots (Chrome, Edge, Brave), read each root's directory, keep the subdirectories whose cookie DB exists at `Network\Cookies` or `Cookies`, sort by directory name, and fill `display_name`/`google_email` from that root's `Local State` via `read_info_cache`:

```rust
pub struct WindowsProfile {
    pub browser: Browser,
    pub profile_dir: String,
    pub display_name: Option<String>,
    pub google_email: Option<String>,
    pub cookies_db: PathBuf,
    pub local_state: PathBuf,
}

const WINDOWS_USER_DATA: [(Browser, [&str; 3]); 3] = [
    (Browser::Chrome, ["Google", "Chrome", "User Data"]),
    (Browser::Edge, ["Microsoft", "Edge", "User Data"]),
    (Browser::Brave, ["BraveSoftware", "Brave-Browser", "User Data"]),
];

pub fn discover_windows_profiles_under(local_app_data: &Path) -> Vec<WindowsProfile> {
    let mut out = Vec::new();
    for (browser, parts) in &WINDOWS_USER_DATA {
        let base = parts.iter().fold(local_app_data.to_path_buf(), |p, part| p.join(part));
        let Ok(entries) = fs::read_dir(&base) else { continue };
        let local_state = base.join("Local State");
        let info_cache = read_info_cache(&local_state);
        let mut found: Vec<(String, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter_map(|dir| {
                let profile = base.join(&dir);
                [profile.join("Network").join("Cookies"), profile.join("Cookies")]
                    .into_iter().find(|p| p.is_file()).map(|db| (dir, db))
            })
            .collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        for (dir, cookies_db) in found {
            let entry = info_cache.as_ref().and_then(|m| m.get(&dir));
            out.push(WindowsProfile {
                browser: browser.clone(),
                display_name: entry.and_then(|e| e.name.clone()),
                google_email: entry.and_then(|e| e.user_name.clone()),
                profile_dir: dir, cookies_db, local_state: local_state.clone(),
            });
        }
    }
    out
}
```

If `Browser` cannot sit in a `const` array, make `WINDOWS_USER_DATA` a small function returning the array.

- [ ] **Step 4: Run**

Run: `cd apps/linux; cargo test -p claude-dashboard-core browser`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/linux/core/src/browser.rs
git commit -m "feat(core): discover Chrome, Edge and Brave profiles on Windows"
```

---

### Task 7: apps/windows workspace and native-messaging framing

**Files:**
- Create: `apps/windows/Cargo.toml`, `apps/windows/rust-toolchain.toml`, `apps/windows/.gitignore`
- Create: `apps/windows/bridge/Cargo.toml`, `apps/windows/bridge/src/lib.rs`, `apps/windows/bridge/src/framing.rs`, `apps/windows/bridge/src/main.rs` (placeholder `fn main() {}` until Task 10)

**Interfaces:**
- Produces: `framing::read_message<R: Read>(r: &mut R) -> Result<Vec<u8>, FramingError>`, `framing::write_message<W: Write>(w: &mut W, body: &[u8]) -> std::io::Result<()>`, `framing::MAX_INBOUND: u32 = 1024 * 1024`, `enum FramingError { Eof, TooLarge(u32), Truncated, Io(std::io::Error) }`.

- [ ] **Step 1: Scaffold the workspace**

`apps/windows/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["bridge"]

[workspace.package]
edition = "2021"
version = "1.18.1"   # kept in sync with /VERSION (sync-version.sh gains this file in sub-project 6)
rust-version = "1.88"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
claude-dashboard-core = { path = "../linux/core" }
```

`apps/windows/rust-toolchain.toml`:

```toml
# Same pin as apps/linux/rust-toolchain.toml; bump both together.
[toolchain]
channel = "1.98.0"
components = ["clippy"]
```

`apps/windows/.gitignore`:

```
/target/
extension/*.pem
```

`apps/windows/bridge/Cargo.toml`:

```toml
[package]
name = "claude-dashboard-bridge"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[[bin]]
name = "claude-dashboard-bridge"
path = "src/main.rs"

[lib]
name = "claude_dashboard_bridge"
path = "src/lib.rs"

[dependencies]
claude-dashboard-core.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid = { version = "1", features = ["v4"] }

[dev-dependencies]
tempfile = "3"
```

`bridge/src/lib.rs`:

```rust
//! Native-messaging host for the Claude Dashboard browser extension: receives
//! one sessionKey message, stores the account, replies, exits.

pub mod framing;
```

`bridge/src/main.rs`: `fn main() {}`

Run: `cd apps/windows; cargo build`
Expected: builds, and `Cargo.lock` is created (commit it — binary workspace).

- [ ] **Step 2: Write the failing tests** — `bridge/src/framing.rs`. Cover: one framed message reads back; empty stdin is `Eof`; a length above `MAX_INBOUND` is `TooLarge` and is rejected before reading the body; a body shorter than its length is `Truncated`; a length prefix under 4 bytes is `Truncated`; `write_message` emits the length then the body.

```rust
//! Chrome native-messaging framing: a 32-bit length in native byte order
//! (little-endian on Windows) followed by that many bytes of UTF-8 JSON.
//! One message in, one out per process.

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(body: &[u8]) -> Vec<u8> {
        [&(body.len() as u32).to_le_bytes()[..], body].concat()
    }

    #[test]
    fn reads_one_message() {
        let mut input = Cursor::new(framed(br#"{"a":1}"#));
        assert_eq!(read_message(&mut input).unwrap(), br#"{"a":1}"#);
    }

    #[test]
    fn empty_stdin_is_eof() {
        assert!(matches!(read_message(&mut Cursor::new(Vec::new())), Err(FramingError::Eof)));
    }

    #[test]
    fn oversized_length_is_rejected() {
        let mut input = Cursor::new((MAX_INBOUND + 1).to_le_bytes().to_vec());
        assert!(matches!(read_message(&mut input), Err(FramingError::TooLarge(n)) if n == MAX_INBOUND + 1));
    }

    #[test]
    fn truncated_body_is_an_error() {
        let mut bytes = 10u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"abc");
        assert!(matches!(read_message(&mut Cursor::new(bytes)), Err(FramingError::Truncated)));
    }

    #[test]
    fn truncated_length_is_an_error() {
        assert!(matches!(read_message(&mut Cursor::new(vec![1u8, 0])), Err(FramingError::Truncated)));
    }

    #[test]
    fn writes_length_then_body() {
        let mut out = Vec::new();
        write_message(&mut out, br#"{"ok":true}"#).unwrap();
        assert_eq!(out, framed(br#"{"ok":true}"#));
    }
}
```

- [ ] **Step 3: Run to confirm it fails**

Run: `cd apps/windows; cargo test -p claude-dashboard-bridge framing`
Expected: FAIL — unresolved `read_message`.

- [ ] **Step 4: Implement** — above the tests. Read the 4-byte length in a loop (0 bytes before any read is `Eof`; 0 bytes partway is `Truncated`; `Interrupted` retries), reject a length over `MAX_INBOUND` before allocating, then `read_exact` the body mapping `UnexpectedEof` to `Truncated`:

```rust
use std::io::{self, Read, Write};

/// Session keys are a few hundred bytes; anything near this is not ours.
pub const MAX_INBOUND: u32 = 1024 * 1024;

#[derive(Debug)]
pub enum FramingError { Eof, TooLarge(u32), Truncated, Io(io::Error) }

pub fn read_message<R: Read>(r: &mut R) -> Result<Vec<u8>, FramingError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut len[got..]) {
            Ok(0) if got == 0 => return Err(FramingError::Eof),
            Ok(0) => return Err(FramingError::Truncated),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FramingError::Io(e)),
        }
    }
    let len = u32::from_le_bytes(len);
    if len > MAX_INBOUND {
        return Err(FramingError::TooLarge(len));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => FramingError::Truncated,
        _ => FramingError::Io(e),
    })?;
    Ok(body)
}

pub fn write_message<W: Write>(w: &mut W, body: &[u8]) -> io::Result<()> {
    w.write_all(&(body.len() as u32).to_le_bytes())?;
    w.write_all(body)?;
    w.flush()
}
```

- [ ] **Step 5: Run, clippy, commit**

```bash
cd apps/windows && cargo test -p claude-dashboard-bridge framing && cargo clippy --all-targets -- -D warnings
git add apps/windows/Cargo.toml apps/windows/Cargo.lock apps/windows/rust-toolchain.toml apps/windows/.gitignore apps/windows/bridge
git commit -m "feat(windows): bridge workspace and native-messaging framing"
```

---

### Task 8: extension-sources.json — bindings and muted installs

**Files:**
- Create: `apps/windows/bridge/src/sources.rs`
- Modify: `apps/windows/bridge/src/lib.rs` (`pub mod sources;`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
  pub struct ExtensionSources {
      #[serde(default)] pub bindings: BTreeMap<String, Binding>, // installId -> binding
      #[serde(default)] pub muted: BTreeSet<String>,            // installIds
  }
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  pub struct Binding { #[serde(rename = "accountId")] pub account_id: String, pub browser: String }
  pub fn sources_path() -> PathBuf
  pub fn load(path: &Path) -> Result<ExtensionSources, String> // missing file -> default
  pub fn save(path: &Path, s: &ExtensionSources) -> Result<(), String> // temp + rename
  impl ExtensionSources { pub fn bind(&mut self, install_id: &str, account_id: &str, browser: &str); pub fn is_muted(&self, install_id: &str) -> bool }
  ```
  The app (sub-projects 2-3) reads `bindings` to tell an extension-sourced account from a pasted one and writes `muted` on delete; callers hold `store::lock_store()` while writing.

- [ ] **Step 1: Write the failing tests** — `sources.rs`. Cover: a missing file loads as default; bindings and mutes round-trip in the documented JSON shape (`{"bindings":{id:{accountId,browser}},"muted":[...]}`); rebinding an install moves it; an unparseable file is an error (never a silent empty list — a corrupt file must not unmute everyone).

```rust
//! extension-sources.json, beside accounts.json: which browser-extension
//! install feeds which account, and which installs the user muted by deleting
//! their account. Kept out of accounts.json so the cross-platform account
//! schema stays unchanged. Shape: contract/windows.md.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_empty() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(&d.path().join("nope.json")).unwrap(), ExtensionSources::default());
    }

    #[test]
    fn bindings_and_mutes_round_trip_in_the_documented_shape() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("sub").join("extension-sources.json");
        let mut s = ExtensionSources::default();
        s.bind("inst-1", "ACC-1", "edge");
        s.muted.insert("inst-2".into());
        save(&path, &s).unwrap();

        assert_eq!(load(&path).unwrap(), s);
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw, serde_json::json!({
            "bindings": { "inst-1": { "accountId": "ACC-1", "browser": "edge" } },
            "muted": ["inst-2"]
        }));
    }

    #[test]
    fn rebinding_an_install_moves_it() {
        let mut s = ExtensionSources::default();
        s.bind("inst-1", "ACC-1", "chrome");
        s.bind("inst-1", "ACC-2", "chrome");
        assert_eq!(s.bindings["inst-1"].account_id, "ACC-2");
        assert!(!s.is_muted("inst-1"));
    }

    #[test]
    fn an_unparseable_file_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("extension-sources.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load(&path).is_err(), "a corrupt file must not silently unmute everything");
    }
}
```

- [ ] **Step 2: Run to confirm it fails**

Run: `cd apps/windows; cargo test -p claude-dashboard-bridge sources`
Expected: FAIL — unresolved items.

- [ ] **Step 3: Implement** — above the tests. `sources_path()` is `store::accounts_path().with_file_name("extension-sources.json")`. `load` returns default on `NotFound`, errors on a parse failure. `save` creates the parent, writes pretty JSON to a temp file and renames. `bind` inserts/replaces; `is_muted` checks the set.

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use claude_dashboard_core::store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionSources {
    #[serde(default)] pub bindings: BTreeMap<String, Binding>,
    #[serde(default)] pub muted: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    #[serde(rename = "accountId")] pub account_id: String,
    pub browser: String,
}

impl ExtensionSources {
    pub fn bind(&mut self, install_id: &str, account_id: &str, browser: &str) {
        self.bindings.insert(install_id.to_string(),
            Binding { account_id: account_id.to_string(), browser: browser.to_string() });
    }
    pub fn is_muted(&self, install_id: &str) -> bool { self.muted.contains(install_id) }
}

pub fn sources_path() -> PathBuf {
    store::accounts_path().with_file_name("extension-sources.json")
}

pub fn load(path: &Path) -> Result<ExtensionSources, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ExtensionSources::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Temp file + rename, like store::save_accounts. Callers hold store::lock_store().
pub fn save(path: &Path, sources: &ExtensionSources) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| fail(&e))?;
    }
    let json = serde_json::to_string_pretty(sources).map_err(|e| fail(&e))?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    fs::write(&tmp, json).map_err(|e| fail(&e))?;
    fs::rename(&tmp, path).map_err(|e| { let _ = fs::remove_file(&tmp); fail(&e) })
}
```

- [ ] **Step 4: Run, commit**

```bash
cd apps/windows && cargo test -p claude-dashboard-bridge sources
git add apps/windows/bridge/src/sources.rs apps/windows/bridge/src/lib.rs
git commit -m "feat(windows): extension-sources.json bindings and muted installs"
```

---

### Task 9: message handler with an injected Environment

**Files:**
- Create: `apps/windows/bridge/src/handler.rs`
- Modify: `apps/windows/bridge/src/lib.rs` (`pub mod handler;`)

**Interfaces:**
- Consumes: `core::key_intake::{apply_session_key, IntakeOutcome}`, `core::manual_key::trimmed_key`, `core::api::ParsedAccount`, `core::plan::ParsedOrg`, `sources::ExtensionSources`.
- Produces:
  ```rust
  pub trait Environment {
      type Guard;
      fn fetch_account(&self, session_key: &str) -> Option<ParsedAccount>;
      fn fetch_orgs(&self, session_key: &str) -> Vec<ParsedOrg>;
      fn lock(&self) -> Result<Self::Guard, String>;
      fn load_accounts(&self) -> Result<Vec<Account>, String>;
      fn save_accounts(&self, accounts: &[Account]) -> Result<(), String>;
      fn load_sources(&self) -> Result<ExtensionSources, String>;
      fn save_sources(&self, sources: &ExtensionSources) -> Result<(), String>;
      fn new_id(&self) -> String;
      fn now_reference(&self) -> f64;
      fn notify_app(&self);
  }
  #[derive(Debug, Clone, PartialEq, Serialize)]
  pub struct Reply { pub ok: bool, pub email: Option<String>, pub error: Option<String>, pub message: Option<String> } // None fields omitted
  pub fn handle(raw: &[u8], env: &impl Environment) -> Reply
  ```
  Error codes: `bad_message`, `no_key`, `muted`, `rejected`, `no_chat_org`, `store`.

**Handler order (documented at the top of the module):**
1. parse + validate the message (no I/O);
2. muted check (before the network);
3. network: `/api/account`, then `/api/organizations` — never under the lock;
4. under `lock()`: load accounts, `apply_session_key`, save accounts, re-check muted, `bind` the install, save sources;
5. after the lock: `notify_app()`.
The session key never appears in a reply.

- [ ] **Step 1: Write the failing tests** — `handler.rs`. Build one in-memory fake `Environment` (a struct holding an optional identity, an orgs body, `RefCell<Vec<Account>>`, `RefCell<ExtensionSources>`, and `Cell` counters for fetches/saves/notify, plus a `fail_save` flag). Then assert each behaviour:

```rust
//! One extension message in, one reply out. See the module-level order above.

#[cfg(test)]
mod tests {
    use super::*;
    // ... FakeEnv defined here (RefCell state + Cell counters) ...

    // a valid new-key message adds the account, replies ok with the email,
    // binds the install, and notifies the app exactly once
    #[test] fn valid_message_adds_binds_and_notifies() { /* ... */ }

    // REVIEW FOCUS 1: a key whose identity matches a pasted account repairs it,
    // leaving exactly one account
    #[test] fn known_identity_is_repaired_not_duplicated() { /* ... */ }

    // REVIEW FOCUS 2: a muted install writes nothing to either store, makes no
    // network call, and replies { ok:false, error:"muted" }
    #[test] fn muted_install_writes_nothing() { /* ... */ }

    // REVIEW FOCUS 3: non-JSON input replies { ok:false, error:"bad_message" }
    #[test] fn non_json_is_bad_message() { /* ... */ }

    // a wrong message type, or a blank sessionKey, is rejected before the network
    #[test] fn missing_fields_are_rejected_before_network() { /* ... */ }

    // an identity the API will not accept replies error:"rejected"
    #[test] fn unaccepted_key_replies_rejected() { /* ... */ }

    // a no-chat-org identity replies error:"no_chat_org" and writes nothing
    #[test] fn no_chat_org_is_reported() { /* ... */ }

    // a store save failure replies error:"store" and does not notify the app
    #[test] fn a_store_failure_is_reported_and_does_not_notify() { /* ... */ }

    // the reply never contains the session key, on any branch
    #[test] fn the_reply_never_echoes_the_key() { /* ... */ }
}
```

Each test builds the message with `serde_json::json!({"type":"sessionKey","installId":"i","browser":"chrome","sessionKey":"sk"}).to_string().into_bytes()` and calls `handle(&bytes, &env)`.

- [ ] **Step 2: Run to confirm it fails**

Run: `cd apps/windows; cargo test -p claude-dashboard-bridge handler`
Expected: FAIL — unresolved `handle`.

- [ ] **Step 3: Implement** — above the tests. Define `Environment`, a `#[derive(Deserialize)] struct Incoming { r#type, installId, browser, sessionKey }` with `#[serde(rename_all = "camelCase")]` plus `type` handled via `r#type`, the `Reply` type (with `#[serde(skip_serializing_if = "Option::is_none")]` on the optional fields), small `Reply::ok(email)` / `Reply::err(code, msg)` constructors, and `handle`:

```rust
use claude_dashboard_core::api::ParsedAccount;
use claude_dashboard_core::key_intake::{apply_session_key, IntakeOutcome};
use claude_dashboard_core::manual_key::trimmed_key;
use claude_dashboard_core::model::Account;
use claude_dashboard_core::plan::ParsedOrg;
use serde::{Deserialize, Serialize};

use crate::sources::ExtensionSources;

pub trait Environment { /* as in Interfaces */ }

#[derive(Deserialize)]
struct Incoming {
    #[serde(rename = "type")] kind: String,
    #[serde(default, rename = "installId")] install_id: String,
    #[serde(default)] browser: String,
    #[serde(default, rename = "sessionKey")] session_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub message: Option<String>,
}

impl Reply {
    fn ok(email: Option<String>) -> Self { Self { ok: true, email, error: None, message: None } }
    fn err(code: &str, msg: &str) -> Self {
        Self { ok: false, email: None, error: Some(code.into()), message: Some(msg.into()) }
    }
}

pub fn handle(raw: &[u8], env: &impl Environment) -> Reply {
    let Ok(msg) = serde_json::from_slice::<Incoming>(raw) else {
        return Reply::err("bad_message", "Could not parse the message.");
    };
    if msg.kind != "sessionKey" {
        return Reply::err("bad_message", "Unknown message type.");
    }
    let Some(session_key) = trimmed_key(&msg.session_key) else {
        return Reply::err("no_key", "No session key in the message.");
    };
    if msg.install_id.is_empty() {
        return Reply::err("bad_message", "No install id.");
    }

    // Muted is checked before the network so a deleted account's extension
    // cannot even cause a fetch. Re-checked again under the lock.
    match env.load_sources() {
        Ok(s) if s.is_muted(&msg.install_id) => return Reply::err("muted", "This account was removed."),
        Ok(_) => {}
        Err(e) => return Reply::err("store", &e),
    }

    let Some(identity) = env.fetch_account(session_key) else {
        return Reply::err("rejected", "The session key was not accepted.");
    };
    let orgs = env.fetch_orgs(session_key);

    let _guard = match env.lock() {
        Ok(g) => g,
        Err(e) => return Reply::err("store", &e),
    };

    // Re-read under the lock: a mute (or any change) may have landed between
    // the pre-network check and here.
    let mut sources = match env.load_sources() {
        Ok(s) => s,
        Err(e) => return Reply::err("store", &e),
    };
    if sources.is_muted(&msg.install_id) {
        return Reply::err("muted", "This account was removed.");
    }
    let mut accounts = match env.load_accounts() {
        Ok(a) => a,
        Err(e) => return Reply::err("store", &e),
    };

    let outcome = apply_session_key(
        &mut accounts, session_key, &identity, &orgs,
        || env.new_id(), env.now_reference(),
    );
    let (account_id, email) = match &outcome {
        IntakeOutcome::RejectNoChatOrg => {
            return Reply::err("no_chat_org", "No organization with chat access.");
        }
        IntakeOutcome::Added { account_id, .. } | IntakeOutcome::Updated { account_id, .. } => {
            (account_id.clone(), identity.email.clone())
        }
    };

    if let Err(e) = env.save_accounts(&accounts) {
        return Reply::err("store", &e);
    }
    sources.bind(&msg.install_id, &account_id, &msg.browser);
    if let Err(e) = env.save_sources(&sources) {
        return Reply::err("store", &e);
    }
    drop(_guard);

    env.notify_app();
    Reply::ok(email)
}
```

- [ ] **Step 4: Run**

Run: `cd apps/windows; cargo test -p claude-dashboard-bridge handler`
Expected: PASS (all handler tests).

- [ ] **Step 5: Clippy, commit**

```bash
cd apps/windows && cargo clippy --all-targets -- -D warnings
git add apps/windows/bridge/src/handler.rs apps/windows/bridge/src/lib.rs
git commit -m "feat(windows): extension message handler over an injected Environment"
```

---

### Task 10: RealEnv, the host binary, manifest, and dev registration

**Files:**
- Create: `apps/windows/bridge/src/real_env.rs`
- Modify: `apps/windows/bridge/src/lib.rs` (`pub mod real_env;`) and `src/main.rs`
- Create: `apps/windows/bridge/com.claude_dashboard.bridge.json`
- Create: `apps/windows/scripts/register-dev-host.ps1`

**Interfaces:**
- Consumes: everything from Tasks 7-9, plus `core::api`, `core::plan::parse_orgs`, `core::store`.
- Produces: `real_env::RealEnv` implementing `handler::Environment`; a working `claude-dashboard-bridge.exe` that processes one message from stdin and writes one reply to stdout; a host manifest; a PowerShell script that registers the manifest for dev.

The network and store wiring here is identical to the Linux helper's `add-key` (same `core::api` calls, same `store` functions), so most of `RealEnv` is a thin pass-through. Only `notify_app` and the lock are Windows-shaped.

- [ ] **Step 1: Implement RealEnv** — `real_env.rs`:

```rust
use claude_dashboard_core::api::{fetch_account, fetch_organizations, parse_account};
use claude_dashboard_core::model::Account;
use claude_dashboard_core::plan::{parse_orgs, ParsedOrg};
use claude_dashboard_core::{api::ParsedAccount, store};
use uuid::Uuid;

use crate::handler::Environment;
use crate::sources::{self, ExtensionSources};

const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0; // 2001-01-01 -> 1970-01-01

pub struct RealEnv;

impl Environment for RealEnv {
    type Guard = store::StoreLock;

    fn fetch_account(&self, session_key: &str) -> Option<ParsedAccount> {
        fetch_account(session_key).ok().and_then(|b| parse_account(&b))
    }
    fn fetch_orgs(&self, session_key: &str) -> Vec<ParsedOrg> {
        fetch_organizations(session_key).ok().map(|b| parse_orgs(&b)).unwrap_or_default()
    }
    fn lock(&self) -> Result<Self::Guard, String> {
        store::lock_store().map_err(|e| e.to_string())
    }
    fn load_accounts(&self) -> Result<Vec<Account>, String> {
        store::load_accounts().map_err(|e| e.to_string())
    }
    fn save_accounts(&self, accounts: &[Account]) -> Result<(), String> {
        store::save_accounts(accounts).map_err(|e| e.to_string())
    }
    fn load_sources(&self) -> Result<ExtensionSources, String> {
        sources::load(&sources::sources_path())
    }
    fn save_sources(&self, s: &ExtensionSources) -> Result<(), String> {
        sources::save(&sources::sources_path(), s)
    }
    fn new_id(&self) -> String { Uuid::new_v4().to_string().to_uppercase() }
    fn now_reference(&self) -> f64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
            - REFERENCE_EPOCH_OFFSET
    }
    fn notify_app(&self) { crate::notify::reload(); }
}
```

- [ ] **Step 2: Implement notify (named pipe, Windows)** — add `pub mod notify;` to `lib.rs` and a `notify.rs`:

```rust
//! Tells a running app to reload the store, via the named pipe it listens on.
//! A missing pipe means the app is not running; that is not an error.

pub const PIPE_NAME: &str = r"\\.\pipe\claude-dashboard";

#[cfg(windows)]
pub fn reload() {
    use std::fs::OpenOptions;
    use std::io::Write;
    if let Ok(mut pipe) = OpenOptions::new().write(true).open(PIPE_NAME) {
        let _ = pipe.write_all(b"reload\n");
    }
}

#[cfg(not(windows))]
pub fn reload() {}
```

(The app's pipe-server side is built in sub-project 2; the bridge only writes.)

- [ ] **Step 3: Implement main** — `src/main.rs` reads one framed message, handles it, writes one framed reply, exits 0. A framing `Eof` (browser disconnected before sending) exits 0 silently; any other framing error writes no reply and exits 1 (nothing to reply to):

```rust
use std::io::{stdin, stdout};

use claude_dashboard_bridge::framing::{read_message, write_message, FramingError};
use claude_dashboard_bridge::handler::handle;
use claude_dashboard_bridge::real_env::RealEnv;

fn main() {
    let raw = match read_message(&mut stdin().lock()) {
        Ok(raw) => raw,
        Err(FramingError::Eof) => return,
        Err(_) => std::process::exit(1),
    };
    let reply = handle(&raw, &RealEnv);
    let body = serde_json::to_vec(&reply).expect("Reply serializes");
    if write_message(&mut stdout().lock(), &body).is_err() {
        std::process::exit(1);
    }
}
```

- [ ] **Step 4: Host manifest template** — `com.claude_dashboard.bridge.json`:

```json
{
  "name": "com.claude_dashboard.bridge",
  "description": "Claude Dashboard native messaging host",
  "path": "claude-dashboard-bridge.exe",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://EXTENSION_ID_PLACEHOLDER/"
  ]
}
```

(`path` is rewritten to an absolute path at install/registration time. The placeholder is replaced with the fixed extension id from Task 11.)

- [ ] **Step 5: Dev registration script** — `scripts/register-dev-host.ps1` takes the built exe path and an extension id, writes a resolved manifest into the bridge's folder, and sets the HKCU registry value Chrome and Edge read for native-messaging hosts (default value = manifest path), under both `HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.claude_dashboard.bridge` and the Edge equivalent. It prints what it wrote. Keep it short and idempotent (`New-Item -Force`).

- [ ] **Step 6: Build and smoke-test the framing end to end**

Run (PowerShell), piping a framed message in and reading the framed reply out is awkward by hand; instead add one integration test `apps/windows/bridge/tests/roundtrip.rs` that spawns the built binary with a `CLAUDE_DASHBOARD_API_BASE` loopback server (as the Linux helper's `support` harness does) and a temp `APPDATA`/`LOCALAPPDATA`, writes a framed message to its stdin, and asserts a framed `{ "ok": true, ... }` comes back and the store now holds the account. Reuse the loopback-server pattern from `apps/linux/helper/tests/support/`.

Run: `cd apps/windows; cargo test -p claude-dashboard-bridge`
Expected: PASS.

- [ ] **Step 7: Clippy, commit**

```bash
cd apps/windows && cargo clippy --all-targets -- -D warnings
git add apps/windows/bridge/src/real_env.rs apps/windows/bridge/src/notify.rs apps/windows/bridge/src/main.rs apps/windows/bridge/src/lib.rs apps/windows/bridge/com.claude_dashboard.bridge.json apps/windows/scripts/register-dev-host.ps1 apps/windows/bridge/tests
git commit -m "feat(windows): bridge binary, named-pipe notify, host manifest and dev registration"
```

---

### Task 11: the browser extension (MV3)

**Files:**
- Create: `apps/windows/extension/manifest.json`, `background.js`, `popup.html`, `popup.js`
- Create: `apps/windows/extension/lib/sync.js`, `lib/status.js`, `lib/extension-id.js`
- Create: `apps/windows/extension/scripts/gen-key.mjs`
- Create: `apps/windows/extension/tests/sync.test.mjs`, `tests/status.test.mjs`

**Interfaces:**
- Produces: a loadable unpacked MV3 extension with a fixed id; `lib/sync.js` exporting `getInstallId(storage)`, `detectBrowser(ua)`, `syncOnce({cookies, runtime, storage})`; `lib/status.js` exporting `describeResult(reply | error)`.

The extension reads only the claude.ai `sessionKey` cookie, through the standard `chrome.cookies` API the user grants on install, and forwards it to the local host. It is the supported, non-privileged counterpart of what the macOS/Linux builds read directly.

- [ ] **Step 1: manifest** — `manifest.json`: MV3, `"minimum_chrome_version": "110"`, permissions `["cookies","nativeMessaging","alarms","storage"]`, host permission `"https://claude.ai/*"`, a service-worker `background.js`, an action popup `popup.html`, and a fixed `"key"` (the base64 public key generated in Step 5) so the unpacked id is stable. Name "Claude Dashboard Sync".

- [ ] **Step 2: failing tests for sync.js** — `tests/sync.test.mjs` under `node --test`, with hand-written mocks for `cookies`, `runtime`, `storage`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { getInstallId, detectBrowser, syncOnce } from '../lib/sync.js';

test('getInstallId mints once and reuses', async () => {
  const bag = {};
  const storage = {
    get: async (k) => ({ [k]: bag[k] }),
    set: async (o) => Object.assign(bag, o),
  };
  const first = await getInstallId(storage);
  const second = await getInstallId(storage);
  assert.equal(first, second);
  assert.match(first, /[0-9a-f-]{36}/);
});

test('detectBrowser reads the UA brand', () => {
  assert.equal(detectBrowser('... Edg/120 ...'), 'edge');
  assert.equal(detectBrowser('... Brave ...'), 'brave');
  assert.equal(detectBrowser('... Chrome/120 ...'), 'chrome');
});

test('syncOnce sends the sessionKey cookie to the host and returns its reply', async () => {
  const sent = [];
  const env = {
    cookies: { get: async () => ({ value: 'sk-ant-sid01-EXT' }) },
    runtime: { sendNativeMessage: async (_host, msg) => { sent.push(msg); return { ok: true, email: 'a@x.com' }; } },
    storage: memStorage(),
  };
  const reply = await syncOnce(env);
  assert.equal(sent[0].type, 'sessionKey');
  assert.equal(sent[0].sessionKey, 'sk-ant-sid01-EXT');
  assert.equal(sent[0].browser, 'chrome');
  assert.ok(sent[0].installId);
  assert.deepEqual(reply, { ok: true, email: 'a@x.com' });
});

test('syncOnce with no cookie does not call the host', async () => {
  let called = false;
  const reply = await syncOnce({
    cookies: { get: async () => null },
    runtime: { sendNativeMessage: async () => { called = true; } },
    storage: memStorage(),
  });
  assert.equal(called, false);
  assert.equal(reply.ok, false);
  assert.equal(reply.error, 'no_cookie');
});
```

(`memStorage` is a tiny in-file helper.)

- [ ] **Step 3: implement lib/sync.js** — `getInstallId` reads/writes `installId` in `storage` (minting a `crypto.randomUUID()`), `detectBrowser` matches `Edg`/`Brave`/`Chrome` in the UA, `syncOnce` reads the `sessionKey` cookie for `https://claude.ai`, returns `{ ok:false, error:'no_cookie' }` when absent, else `sendNativeMessage('com.claude_dashboard.bridge', {type,installId,browser,sessionKey})` and returns the reply.

- [ ] **Step 4: REVIEW FOCUS 5 — status + background** — `tests/status.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { describeResult } from '../lib/status.js';

test('a successful sync shows the email and no badge', () => {
  const s = describeResult({ ok: true, email: 'a@x.com' });
  assert.equal(s.badge, '');
  assert.match(s.text, /a@x\.com/);
});

test('unreachable_host_sets_badge_and_status', () => {
  // sendNativeMessage rejects when the host is not registered / app not installed
  const s = describeResult(new Error('Specified native messaging host not found.'));
  assert.equal(s.badge, '!');
  assert.match(s.text, /install|not found|Claude Dashboard/i);
});

test('a host error reply surfaces its message', () => {
  const s = describeResult({ ok: false, error: 'muted', message: 'This account was removed.' });
  assert.equal(s.badge, '!');
  assert.match(s.text, /removed/);
});
```

Implement `lib/status.js` to map a thrown error, an `{ok:false}` reply, and an `{ok:true}` reply to `{ badge, text }`. `background.js` wires it up: run `syncOnce` on install, on `cookies.onChanged` for the `sessionKey` cookie, and on a 30-minute `alarms` tick; set the action badge and title from `describeResult`. `popup.js` shows the last status text.

- [ ] **Step 5: fixed extension id** — `scripts/gen-key.mjs` generates an RSA keypair, prints the base64 SPKI public key for `manifest.json`'s `"key"` and the derived extension id; `lib/extension-id.js` exports `extensionIdFromKey(b64)` (SHA-256 of the DER, first 16 bytes mapped to a–p) with a unit test pinning one known key→id pair. Run it once, paste the `"key"` into the manifest and the id into `com.claude_dashboard.bridge.json`'s `allowed_origins` (replacing the placeholder from Task 10). Keep the generated `.pem` out of git (already in `.gitignore`).

- [ ] **Step 6: run the extension tests**

Run: `cd apps/windows/extension; node --test`
Expected: PASS (sync, status, extension-id).

- [ ] **Step 7: manual smoke test (documented, not automated)** — in `docs`: load the unpacked extension, run `register-dev-host.ps1` with the built exe and the fixed id, open claude.ai logged in, confirm the popup shows "Synced <email>" and the account appears in the store. This is a human step; note it in `contract/windows.md`.

- [ ] **Step 8: commit**

```bash
git add apps/windows/extension
git commit -m "feat(windows): MV3 extension reading the sessionKey cookie via the host"
```

---

### Task 12: contract/windows.md, CI, and CLAUDE.md

**Files:**
- Create: `contract/windows.md`
- Create: `.github/workflows/ci.yml`
- Modify: `CLAUDE.md`

**Interfaces:** none (docs and automation).

- [ ] **Step 1: Write contract/windows.md** — document, as fixed cross-process behaviour:
  - **Data paths:** `%APPDATA%\claude-dashboard\accounts.json`, `%LOCALAPPDATA%\claude-dashboard\usage_logs.db`, `extension-sources.json` beside `accounts.json`, `accounts.json.lock`.
  - **At-rest key protection:** per-user (`CryptProtectData`), base64 wire shape, not portable between machines or OSes.
  - **Cookie formats:** `v10`/`v11` decoded with the Local State key; `v20` is app-bound and is never opened (the extension is the path for those).
  - **Native-messaging message + reply shapes** and the full error-code list (`bad_message`, `no_key`, `muted`, `rejected`, `no_chat_org`, `store`).
  - **extension-sources.json shape** (`bindings`, `muted`) and the muted rule: deleting an extension-sourced account mutes its install id.
  - **Named pipe** `\\.\pipe\claude-dashboard`, message `reload`.
  - **Host name** `com.claude_dashboard.bridge` and the HKCU registry locations.
  - **Manual smoke test** steps from Task 11 Step 7.
  Note at the top that account business rules (plan, dedupe, org selection, burn rate, Fable) are unchanged and remain governed by the existing `contract/` files, now exercised on Windows too.

- [ ] **Step 2: CI workflow** — `.github/workflows/ci.yml` with two jobs on push/PR:
  - `linux` (ubuntu-latest): `cd apps/linux && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`. This is what proves the helper's byte-exact contract still holds after the Task 4 refactor.
  - `windows` (windows-latest): `cd apps/linux && cargo test -p claude-dashboard-core`; then `cd apps/windows && cargo test --workspace && cargo clippy --all-targets -- -D warnings`; then `cd apps/windows/extension && node --test`.
  Pin `dtolnay/rust-toolchain@1.98.0` (or rely on `rust-toolchain.toml`) and `actions/setup-node@v4` with Node 20.

- [ ] **Step 3: Run the workflow logic locally where possible**

Run (on this Windows machine): `cd apps/linux; cargo test -p claude-dashboard-core; cd ../windows; cargo test --workspace; cd extension; node --test`
Expected: all PASS. (The `linux` job's helper tests run in CI, not here.)

- [ ] **Step 4: Update CLAUDE.md** — add a short "Windows (apps/windows/)" subsection under Architecture mirroring the Linux one: the `apps/windows/` workspace (app in a later sub-project, bridge, extension), that it reuses `apps/linux/core`, and the Windows-specific `core` modules (`userprotect`, `cookie/win`, Windows store paths). Add the Windows build/test commands:

```bash
# Core on Windows
cd apps/linux && cargo test -p claude-dashboard-core
# Bridge + sources + handler + framing
cd apps/windows && cargo test --workspace
# Extension
cd apps/windows/extension && node --test
```

- [ ] **Step 5: Commit**

```bash
git add contract/windows.md .github/workflows/ci.yml CLAUDE.md
git commit -m "docs(windows): contract, CI workflow, and CLAUDE.md for the Windows core and bridge"
```

---

## Done when

- `cargo test -p claude-dashboard-core` passes on Windows and the full `apps/linux` workspace still passes on Linux/macOS (CI's `linux` job).
- `apps/windows` (bridge: framing, sources, handler, roundtrip) and the extension tests pass.
- A human smoke test (Task 11 Step 7) confirms a real claude.ai login flows extension -> bridge -> store and the account is readable, with the session key never appearing in any log or reply.

Sub-project 2 (the Slint app shell) consumes: `core` on Windows, `store::lock_store`, `cookie::win` + `browser::discover_windows_profiles_under` (for the scan tab), the named pipe the bridge writes to, and `extension-sources.json` to distinguish extension-sourced accounts.
