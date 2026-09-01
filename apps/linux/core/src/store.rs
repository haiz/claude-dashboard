//! Accounts JSON persistence, at-rest session-key encryption, and the
//! usage-log SQLite time series.
//!
//! Mirrors three macOS sources:
//! - `apps/macos/ClaudeDashboard/Services/AccountStore.swift` (accounts
//!   JSON persistence — see `contract/account-schema.md` for the wire
//!   shape, which lives entirely in `crate::model::Account`'s serde derive;
//!   this module only owns *where* the JSON lives and *when* it's absent).
//! - `apps/macos/Shared/CryptoService.swift` (at-rest session-key
//!   encryption — platform detail, not contract; see "Linux-local at-rest
//!   scheme" below).
//! - `apps/macos/ClaudeDashboard/Services/UsageLogStore.swift` (usage-log
//!   SQLite time series — see `contract/usage-log.md` for the exact schema,
//!   column semantics, and the three-condition compression check this
//!   module reproduces verbatim).

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use hkdf::Hkdf;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::Sha256;

use crate::model::Account;

/// Errors from accounts JSON I/O or usage-log SQLite operations.
#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "I/O error: {e}"),
            StoreError::Json(e) => write!(f, "JSON error: {e}"),
            StoreError::Sqlite(e) => write!(f, "SQLite error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Json(e)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

// ---------------------------------------------------------------------
// Accounts JSON
// ---------------------------------------------------------------------

fn home_dir() -> PathBuf {
    env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
}

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

/// `$XDG_CONFIG_HOME` (or `~/.config`) + `/claude-dashboard/accounts.json`.
pub fn accounts_path() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", &[".config"]).join("claude-dashboard").join("accounts.json")
}

/// `$XDG_DATA_HOME` (or `~/.local/share`) + `/claude-dashboard/usage_logs.db`.
pub fn usage_log_path() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", &[".local", "share"]).join("claude-dashboard").join("usage_logs.db")
}

/// Loads the accounts array. A missing file is not an error — it means no
/// accounts have been saved yet, so this returns an empty `Vec`.
pub fn load_accounts() -> Result<Vec<Account>, StoreError> {
    let path = accounts_path();
    match fs::read_to_string(&path) {
        Ok(contents) => Ok(Account::from_json(&contents)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

/// Saves the accounts array, creating the parent directory if needed. The
/// wire encoding is `Account`'s own serde derive (Task 2) — this function
/// only owns the file path and directory creation.
pub fn save_accounts(accounts: &[Account]) -> Result<(), StoreError> {
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = Account::to_json_array(accounts)?;
    fs::write(&path, json)?;
    Ok(())
}

// ---------------------------------------------------------------------
// At-rest session-key encryption — Linux-local scheme
// ---------------------------------------------------------------------
//
// macOS's `CryptoService` (`apps/macos/Shared/CryptoService.swift`) derives
// its AES-GCM key via HKDF-SHA256 seeded from the machine's
// `IOPlatformUUID`, an IOKit-only identifier with no Linux equivalent (see
// `contract/account-schema.md`'s "sessionKey is not portable" section,
// which explicitly defers this choice to each platform port).
//
// This is the Linux analogue, not a byte-compatible reimplementation:
// - Key material: HKDF-SHA256 over the contents of `/etc/machine-id`
//   (falling back to `/var/lib/dbus/machine-id`, the same value on systems
//   that symlink the two, and finally to a fixed constant on a system with
//   neither — e.g. a minimal container — so encryption never panics, it
//   just stops being host-bound in that degenerate case).
// - Sealed box layout: 12-byte GCM nonce prepended to ciphertext+tag,
//   base64-encoded as one string — the AES-GCM "combined representation"
//   CryptoKit also uses, but produced by different key material and a
//   different concrete implementation, so a value encrypted by one
//   platform's scheme cannot be decrypted by the other's. That is
//   intentional: `sessionKey` was never a portable value (see the contract
//   note above) and this port does not need to make it one.

const HKDF_SALT: &[u8] = b"com.claude-dashboard.v1";

fn machine_id_bytes() -> Vec<u8> {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(s) = fs::read_to_string(path) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return trimmed.as_bytes().to_vec();
            }
        }
    }
    // No machine id readable (e.g. a minimal container). Encryption still
    // works, it just isn't bound to this specific host.
    b"claude-dashboard-linux-fallback-machine-id".to_vec()
}

fn derive_key() -> [u8; 32] {
    let ikm = machine_id_bytes();
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), &ikm);
    let mut okm = [0u8; 32];
    hk.expand(&[], &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

/// Seals `plain` with AES-256-GCM, keyed by [`derive_key`], and returns
/// `base64(nonce || ciphertext || tag)`.
pub fn encrypt_session_key(plain: &str) -> String {
    let key_bytes = derive_key();
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plain.as_bytes())
        .expect("AES-GCM encryption with a freshly generated nonce does not fail");

    let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);
    BASE64.encode(combined)
}

/// Reverses [`encrypt_session_key`]. Returns `None` on any malformed input
/// or decryption failure (wrong host, corrupted value, truncated data) —
/// there is no partial result to salvage.
pub fn decrypt_session_key(cipher_b64: &str) -> Option<String> {
    let combined = BASE64.decode(cipher_b64).ok()?;
    if combined.len() < 12 {
        return None;
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);

    let key_bytes = derive_key();
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher.decrypt(nonce, ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

// ---------------------------------------------------------------------
// Usage log — SQLite time series with compression
// ---------------------------------------------------------------------

const SCHEMA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS accounts_map (
        aid INTEGER PRIMARY KEY AUTOINCREMENT,
        account_id TEXT NOT NULL UNIQUE
    );
    CREATE TABLE IF NOT EXISTS usage_logs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        aid INTEGER NOT NULL,
        w INTEGER NOT NULL,
        rat INTEGER NOT NULL,
        t INTEGER NOT NULL,
        u INTEGER NOT NULL,
        lim INTEGER DEFAULT 0
    );
    CREATE INDEX IF NOT EXISTS idx_logs_lookup ON usage_logs(aid, w, rat, t);
";

/// A local time-series log of polled usage percentages. See
/// `contract/usage-log.md` for the schema and compression policy this type
/// reproduces exactly; the SQLite file layout itself is not contract.
pub struct UsageLogStore {
    conn: Connection,
    /// Caches `account_id -> aid` lookups/inserts against `accounts_map`
    /// within this store's lifetime, avoiding a round trip on every
    /// `record_at` call for the common case of repeatedly logging the same
    /// account.
    aid_cache: HashMap<String, i64>,
}

impl UsageLogStore {
    /// Opens an in-memory database — for tests only (a fresh, empty store
    /// with no on-disk footprint).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn)
    }

    /// Opens (creating if needed) the real on-disk database at
    /// [`usage_log_path`].
    pub fn open() -> Result<Self, StoreError> {
        let path = usage_log_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn, aid_cache: HashMap::new() })
    }

    /// Looks up `account_id`'s `aid` in `accounts_map`, inserting a new row
    /// if none exists yet. Mirrors `resolveAccountId`
    /// (`UsageLogStore.swift:278-290`). Returns `None` only if the
    /// underlying SQLite operations fail, which `record_at` treats as "skip
    /// this poll" — the same silent-failure behaviour as the Swift source's
    /// `guard let aid = resolveAccountId(accountId) else { return }`.
    fn resolve_aid(&mut self, account_id: &str) -> Option<i64> {
        if let Some(&aid) = self.aid_cache.get(account_id) {
            return Some(aid);
        }
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT aid FROM accounts_map WHERE account_id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .optional()
            .ok()?;
        let aid = match existing {
            Some(aid) => aid,
            None => {
                self.conn
                    .execute("INSERT INTO accounts_map (account_id) VALUES (?1)", params![account_id])
                    .ok()?;
                self.conn.last_insert_rowid()
            }
        };
        self.aid_cache.insert(account_id.to_string(), aid);
        Some(aid)
    }

    /// The two most recent existing rows for the exact `(aid, w, rat)`
    /// triple, ordered by `t DESC` (most recent first) — the compression
    /// check's input.
    fn recent_two(&self, aid: i64, w: i64, rat: i64) -> rusqlite::Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, u FROM usage_logs WHERE aid = ?1 AND w = ?2 AND rat = ?3 \
             ORDER BY t DESC LIMIT 2",
        )?;
        let rows = stmt
            .query_map(params![aid, w, rat], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect();
        rows
    }

    /// `applyCompression` (`UsageLogStore.swift:248-274` /
    /// `contract/usage-log.md` "Compression policy"), applied before every
    /// insert. The exact three-condition check, scoped to `(aid, w, rat)`:
    /// if at least 2 rows exist, their `u` values are equal to each other,
    /// AND that shared value equals the incoming `u`, delete the
    /// more-recent of the two existing rows (it becomes the run's middle
    /// entry once the new row lands, and middle entries of a plateau don't
    /// survive). A SQLite failure here is swallowed — same effect as the
    /// Swift `guard ... else { return }` chain: no compression happens, the
    /// insert proceeds normally.
    fn apply_compression(&mut self, aid: i64, w: i64, rat: i64, u: i64) {
        let Ok(recent) = self.recent_two(aid, w, rat) else { return };
        if recent.len() >= 2 && recent[0].1 == recent[1].1 && recent[0].1 == u {
            let _ = self.conn.execute("DELETE FROM usage_logs WHERE id = ?1", params![recent[0].0]);
        }
    }

    /// Records one poll, using the current wall-clock time as `t`. See
    /// [`Self::record_at`] for the full column semantics.
    pub fn record(&mut self, account_id: &str, window: i64, resets_at: f64, utilization: f64, is_limited: bool) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.record_at(account_id, window, resets_at, utilization, is_limited, now);
    }

    /// Records one poll with an injectable `now` (Unix seconds, fractional)
    /// so tests can control `t` precisely. Column semantics
    /// (`contract/usage-log.md`):
    /// - `rat` = `resets_at as i64` — truncated toward zero.
    /// - `t` = `now as i64` — truncated toward zero.
    /// - `u` = `(utilization * 100.0).round() as i64` — **rounded**,
    ///   round-half-away-from-zero (Rust's `f64::round`, matching Swift's
    ///   `round()`), not truncated.
    /// - `lim` = `1` if `is_limited` else `0`.
    ///
    /// Compression (see [`Self::apply_compression`]) runs before the insert,
    /// exactly as in the Swift source. Any SQLite failure — resolving the
    /// account's `aid`, or the insert itself — causes this poll to be
    /// silently skipped, mirroring the Swift source's guard-and-return
    /// error handling rather than propagating a `Result` callers would have
    /// to decide how to react to for what is, in both apps, a best-effort
    /// background log.
    pub fn record_at(
        &mut self,
        account_id: &str,
        window: i64,
        resets_at: f64,
        utilization: f64,
        is_limited: bool,
        now: f64,
    ) {
        let Some(aid) = self.resolve_aid(account_id) else { return };
        let rat = resets_at as i64;
        let t = now as i64;
        let u = (utilization * 100.0).round() as i64;
        let lim: i64 = if is_limited { 1 } else { 0 };

        self.apply_compression(aid, window, rat, u);

        let _ = self.conn.execute(
            "INSERT INTO usage_logs (aid, w, rat, t, u, lim) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![aid, window, rat, t, u, lim],
        );
    }

    /// Test helper: total row count in `usage_logs` for `account_id` in
    /// `window`, across all `rat` values. Returns `0` if the account has no
    /// mapped `aid` yet or the query fails.
    pub fn count(&self, account_id: &str, window: i64) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM usage_logs l JOIN accounts_map m ON m.aid = l.aid \
                 WHERE m.account_id = ?1 AND l.w = ?2",
                params![account_id, window],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }

    /// Test helper: the stored `u` (raw, still `* 100`) of the most
    /// recently recorded row for `account_id` in `window`. Returns `0` if
    /// no such row exists.
    pub fn raw_u(&self, account_id: &str, window: i64) -> i64 {
        self.conn
            .query_row(
                "SELECT l.u FROM usage_logs l JOIN accounts_map m ON m.aid = l.aid \
                 WHERE m.account_id = ?1 AND l.w = ?2 ORDER BY l.t DESC, l.id DESC LIMIT 1",
                params![account_id, window],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn mem_store() -> UsageLogStore {
        UsageLogStore::open_in_memory().unwrap()
    }

    /// `accounts_path()` reads the process-global `XDG_CONFIG_HOME` env var,
    /// which `cargo test`'s default multi-threaded runner shares across
    /// every test in this binary. Any test that sets it must hold this lock
    /// for its whole body so two such tests never interleave their
    /// set-var/save/load sequences.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn three_identical_values_keep_first_and_last() {
        let mut s = mem_store();
        for t in [10.0, 20.0, 30.0] {
            s.record_at("ACC", 0, 1000.0, 42.0, false, t);
        }
        assert_eq!(s.count("ACC", 0), 2);
    }

    #[test]
    fn four_identical_values_still_two_rows() {
        let mut s = mem_store();
        for t in [10.0, 20.0, 30.0, 40.0] {
            s.record_at("ACC", 0, 1000.0, 42.0, false, t);
        }
        assert_eq!(s.count("ACC", 0), 2);
    }

    #[test]
    fn value_changes_no_compression() {
        let mut s = mem_store();
        s.record_at("ACC", 0, 1000.0, 10.0, false, 10.0);
        s.record_at("ACC", 0, 1000.0, 20.0, false, 20.0);
        s.record_at("ACC", 0, 1000.0, 30.0, false, 30.0);
        assert_eq!(s.count("ACC", 0), 3);
    }

    #[test]
    fn plateau_then_change_keeps_three() {
        // Mirrors testSmartCompression_plateauThenChange: three identical
        // then one different -> 3 rows (first+last of the plateau, plus
        // the new value).
        let mut s = mem_store();
        s.record_at("ACC", 0, 1000.0, 42.0, false, 10.0);
        s.record_at("ACC", 0, 1000.0, 42.0, false, 20.0);
        s.record_at("ACC", 0, 1000.0, 42.0, false, 30.0);
        s.record_at("ACC", 0, 1000.0, 99.0, false, 40.0);
        assert_eq!(s.count("ACC", 0), 3);
    }

    #[test]
    fn compression_is_scoped_to_exact_rat() {
        // A new `rat` starts a fresh run even if `u` repeats — compression
        // must not fire across a reset-at change.
        let mut s = mem_store();
        s.record_at("ACC", 0, 1000.0, 42.0, false, 10.0);
        s.record_at("ACC", 0, 1000.0, 42.0, false, 20.0);
        s.record_at("ACC", 0, 2000.0, 42.0, false, 30.0);
        assert_eq!(s.count("ACC", 0), 3);
    }

    #[test]
    fn u_is_rounded_not_truncated() {
        let mut s = mem_store();
        s.record_at("ACC", 0, 1000.0, 45.005, false, 10.0);
        assert_eq!(s.raw_u("ACC", 0), 4501); // round, not 4500
    }

    #[test]
    fn rat_and_t_are_truncated_toward_zero() {
        let mut s = mem_store();
        s.record_at("ACC", 0, 1000.9, 1.0, false, 10.9);
        let (rat, t): (i64, i64) = s
            .conn
            .query_row("SELECT rat, t FROM usage_logs LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(rat, 1000);
        assert_eq!(t, 10);
    }

    #[test]
    fn lim_is_stored_as_boolean_int() {
        let mut s = mem_store();
        s.record_at("ACC", 0, 1000.0, 1.0, true, 10.0);
        let lim: i64 = s.conn.query_row("SELECT lim FROM usage_logs LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(lim, 1);
    }

    #[test]
    fn window_raw_values_are_distinct_and_never_two() {
        // fiveHour=0, sevenDay=1, fable=3 -- 2 is retired Sonnet and must
        // never be produced by this store.
        let mut s = mem_store();
        s.record_at("ACC", 0, 1000.0, 1.0, false, 10.0);
        s.record_at("ACC", 1, 1000.0, 1.0, false, 10.0);
        s.record_at("ACC", 3, 1000.0, 1.0, false, 10.0);
        assert_eq!(s.count("ACC", 0), 1);
        assert_eq!(s.count("ACC", 1), 1);
        assert_eq!(s.count("ACC", 3), 1);
        assert_eq!(s.count("ACC", 2), 0);
    }

    #[test]
    fn missing_accounts_file_loads_empty() {
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        env::set_var("XDG_CONFIG_HOME", dir.path());
        let accounts = load_accounts().unwrap();
        assert!(accounts.is_empty());
    }

    #[test]
    fn accounts_roundtrip_reference_epoch_and_uuid() {
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        let a: Vec<Account> = serde_json::from_str(
            r#"[{"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"x",
                 "chromeProfilePath":"/p","plan":"Pro","status":"active","lastSynced":0.0}]"#,
        )
        .unwrap();
        save_accounts(&a).unwrap();
        let back = load_accounts().unwrap();
        assert_eq!(back[0].id, "3B8C3678-3A00-425C-8D22-22BCA37AE65B");
        assert_eq!(back[0].last_synced, Some(0.0));
    }

    #[test]
    fn accounts_array_roundtrip_multi_account() {
        // Exercises Account::from_json/to_json_array's array round-trip
        // (deferred from Task 2) with more than one account.
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        let a: Vec<Account> = serde_json::from_str(
            r#"[
                {"id":"3B8C3678-3A00-425C-8D22-22BCA37AE65B","name":"one",
                 "chromeProfilePath":"/p1","plan":"Pro","status":"active"},
                {"id":"4C9D4789-4B11-536D-9E33-33CDB48BF76C","name":"two",
                 "email":"two@example.com","chromeProfilePath":"/p2",
                 "orgId":"org-2","plan":"Max 20x","status":"expired","isPinned":true}
            ]"#,
        )
        .unwrap();
        save_accounts(&a).unwrap();
        let back = load_accounts().unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "one");
        assert_eq!(back[1].name, "two");
        assert_eq!(back[1].email.as_deref(), Some("two@example.com"));
        assert_eq!(back[1].org_id.as_deref(), Some("org-2"));
        assert!(back[1].is_pinned);
    }

    #[test]
    fn session_key_encrypt_decrypt_roundtrip() {
        let plain = "sk-ant-sid01-super-secret-value";
        let cipher = encrypt_session_key(plain);
        assert_ne!(cipher, plain);
        assert_eq!(decrypt_session_key(&cipher).as_deref(), Some(plain));
    }

    #[test]
    fn session_key_decrypt_rejects_garbage() {
        assert_eq!(decrypt_session_key("not-valid-base64!!"), None);
        assert_eq!(decrypt_session_key(&BASE64.encode(b"too short")), None);
    }
}
