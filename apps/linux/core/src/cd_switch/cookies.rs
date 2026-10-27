//! Claude Desktop's claude.ai web session (port of macOS `ClaudeDesktopCookieFile.swift`).
//! Desktop signs in as the account of the `sessionKey` cookie in its Chromium cookie
//! database, not the account named in `config.json`, so a switch must replace it too.
//! Values are `v10` AES-256-GCM under the safe-storage key; meta version 24+ prefixes the
//! plaintext with sha256 of the host.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use rusqlite::{params, Connection, OpenFlags};

use crate::browser::{chrome_time, read_schema_version};
use crate::cookie::gcm_seal;
use crate::cookie::win::decode_value;

const HOST: &str = ".claude.ai";
/// Long enough to outlive any switch; claude.ai renews the cookie on use.
const LIFETIME_SECS: f64 = 30.0 * 86400.0;

pub trait DesktopSession {
    /// The `sessionKey` Desktop holds; `None` when the database or the row is absent,
    /// unreadable or app-bound (`v20`).
    fn read_session_key(&self) -> Option<String>;
    /// Replaces the claude.ai session with `session_key` and `lastActiveOrg`, and drops the
    /// account-bound `routingHint` and `sessionKeyLC` rows (claude.ai issues them again).
    /// Errors when the database is absent or the write fails; Desktop must not be running.
    fn write_session(&self, session_key: &str, org_uuid: &str) -> Result<(), String>;
}

/// Desktop's Chromium cookie database with the safe-storage key; the clock is injected so
/// tests never depend on the wall time.
pub struct FileDesktopSession {
    pub path: PathBuf,
    pub key: [u8; 32],
    pub now_unix: fn() -> f64,
}

impl FileDesktopSession {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs_f64((self.now_unix)().max(0.0))
    }

    fn seal(&self, value: &str, prefixed: bool) -> Vec<u8> {
        let mut out = b"v10".to_vec();
        out.extend(gcm_seal(&self.key, value.as_bytes(), prefixed.then_some(HOST)));
        out
    }
}

impl DesktopSession for FileDesktopSession {
    fn read_session_key(&self) -> Option<String> {
        if !self.path.exists() {
            return None;
        }
        let conn = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        conn.busy_timeout(Duration::from_secs(2)).ok()?;
        let schema = read_schema_version(&conn).unwrap_or(24);
        let (host, blob): (String, Vec<u8>) = conn
            .query_row(
                "SELECT host_key, encrypted_value FROM cookies \
                 WHERE host_key IN ('.claude.ai', 'claude.ai') AND name = 'sessionKey' \
                 AND (expires_utc = 0 OR expires_utc > ?1) \
                 ORDER BY expires_utc DESC LIMIT 1",
                [chrome_time(self.now())],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()?;
        decode_value(&blob, &host, schema, &self.key).ok()
    }

    fn write_session(&self, session_key: &str, org_uuid: &str) -> Result<(), String> {
        if !self.path.exists() {
            return Err("no cookie database".into());
        }
        let mut conn = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e| format!("open: {e}"))?;
        conn.busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let prefixed = read_schema_version(&conn).unwrap_or(0) >= 24;
        let session_blob = self.seal(session_key, prefixed);
        let org_blob = self.seal(org_uuid, prefixed);
        let stamp = chrome_time(self.now());
        let expires = chrome_time(self.now() + Duration::from_secs_f64(LIFETIME_SECS));

        // Dropping the transaction without commit rolls back.
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM cookies WHERE host_key IN ('.claude.ai', 'claude.ai') \
             AND name IN ('sessionKey', 'lastActiveOrg', 'routingHint', 'sessionKeyLC')",
            [],
        )
        .map_err(|e| e.to_string())?;
        // sessionKey: Secure, HttpOnly, Lax, set by the server; lastActiveOrg is script-readable.
        insert(&tx, "sessionKey", &session_blob, true, 1, stamp, expires)?;
        insert(&tx, "lastActiveOrg", &org_blob, false, 2, stamp, expires)?;
        tx.commit().map_err(|e| e.to_string())
    }
}

fn insert(
    tx: &rusqlite::Transaction<'_>,
    name: &str,
    blob: &[u8],
    http_only: bool,
    source_type: i64,
    stamp: i64,
    expires: i64,
) -> Result<(), String> {
    tx.execute(
        "INSERT INTO cookies (creation_utc, host_key, top_frame_site_key, name, value, \
           encrypted_value, path, expires_utc, is_secure, is_httponly, last_access_utc, \
           has_expires, is_persistent, priority, samesite, source_scheme, source_port, \
           last_update_utc, source_type, has_cross_site_ancestor) \
         VALUES (?1, '.claude.ai', '', ?2, '', ?3, '/', ?4, 1, ?5, ?1, 1, 1, 1, 1, 2, 443, ?1, ?6, 1)",
        params![stamp, name, blob, expires, http_only as i64, source_type],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cookie::gcm_open;

    const KEY: [u8; 32] = [3u8; 32];
    const NOW: f64 = 1_800_000_000.0;
    fn now() -> f64 {
        NOW
    }

    const SCHEMA: &str = "CREATE TABLE meta(key TEXT, value TEXT);
        CREATE TABLE cookies(creation_utc INTEGER, host_key TEXT, top_frame_site_key TEXT, name TEXT,
          value TEXT, encrypted_value BLOB, path TEXT, expires_utc INTEGER, is_secure INTEGER,
          is_httponly INTEGER, last_access_utc INTEGER, has_expires INTEGER, is_persistent INTEGER,
          priority INTEGER, samesite INTEGER, source_scheme INTEGER, source_port INTEGER,
          last_update_utc INTEGER, source_type INTEGER, has_cross_site_ancestor INTEGER);";

    fn fixture(version: &str) -> (tempfile::TempDir, FileDesktopSession) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Cookies");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute("INSERT INTO meta VALUES('version', ?1)", [version])
            .unwrap();
        (dir, FileDesktopSession { path, key: KEY, now_unix: now })
    }

    fn seed(s: &FileDesktopSession, host: &str, name: &str) {
        let conn = Connection::open(&s.path).unwrap();
        conn.execute(
            "INSERT INTO cookies (host_key, name, encrypted_value, expires_utc) VALUES (?1, ?2, x'00', 0)",
            params![host, name],
        )
        .unwrap();
    }

    fn names(s: &FileDesktopSession) -> Vec<(String, String)> {
        let conn = Connection::open(&s.path).unwrap();
        let mut st = conn
            .prepare("SELECT host_key, name FROM cookies ORDER BY host_key, name")
            .unwrap();
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn write_then_read_round_trips() {
        let (_d, s) = fixture("24");
        s.write_session("sk-ant-sid01-abc", "org-1").unwrap();
        assert_eq!(s.read_session_key().as_deref(), Some("sk-ant-sid01-abc"));
    }

    #[test]
    fn write_replaces_session_and_drops_routing_hint_and_session_key_lc() {
        let (_d, s) = fixture("24");
        for host in [".claude.ai", "claude.ai"] {
            for n in ["sessionKey", "lastActiveOrg", "routingHint", "sessionKeyLC"] {
                seed(&s, host, n);
            }
        }
        seed(&s, ".other.com", "sessionKey");
        s.write_session("new", "org-2").unwrap();
        assert_eq!(
            names(&s),
            vec![
                (".claude.ai".into(), "lastActiveOrg".into()),
                (".claude.ai".into(), "sessionKey".into()),
                (".other.com".into(), "sessionKey".into()),
            ]
        );
    }

    #[test]
    fn session_key_row_is_httponly_source_type_1_and_last_active_org_is_not() {
        let (_d, s) = fixture("24");
        s.write_session("k", "o").unwrap();
        let conn = Connection::open(&s.path).unwrap();
        let get = |n: &str| -> (i64, i64) {
            conn.query_row(
                "SELECT is_httponly, source_type FROM cookies WHERE name = ?1",
                [n],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(get("sessionKey"), (1, 1));
        assert_eq!(get("lastActiveOrg"), (0, 2));
    }

    #[test]
    fn expires_is_30_days_after_now() {
        let (_d, s) = fixture("24");
        s.write_session("k", "o").unwrap();
        let conn = Connection::open(&s.path).unwrap();
        let (creation, expires): (i64, i64) = conn
            .query_row(
                "SELECT creation_utc, expires_utc FROM cookies WHERE name = 'sessionKey'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(creation, (NOW as i64 + 11_644_473_600) * 1_000_000);
        assert_eq!(expires - creation, 30 * 86400 * 1_000_000);
    }

    #[test]
    fn schema_below_24_has_no_host_prefix() {
        let (_d, s) = fixture("23");
        s.write_session("bare", "o").unwrap();
        let conn = Connection::open(&s.path).unwrap();
        let blob: Vec<u8> = conn
            .query_row(
                "SELECT encrypted_value FROM cookies WHERE name = 'sessionKey'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(&blob[..3], b"v10");
        assert_eq!(gcm_open(&KEY, &blob[3..], ".claude.ai", 0).unwrap(), "bare");
        assert_eq!(s.read_session_key().as_deref(), Some("bare"));
    }

    #[test]
    fn missing_db_read_none_and_write_errors() {
        let dir = tempfile::tempdir().unwrap();
        let s = FileDesktopSession { path: dir.path().join("nope"), key: KEY, now_unix: now };
        assert_eq!(s.read_session_key(), None);
        assert!(s.write_session("k", "o").is_err());
    }

    #[test]
    fn v20_value_reads_none() {
        let (_d, s) = fixture("24");
        let conn = Connection::open(&s.path).unwrap();
        conn.execute(
            "INSERT INTO cookies (host_key, name, encrypted_value, expires_utc) \
             VALUES ('.claude.ai', 'sessionKey', ?1, 0)",
            params![[b"v20".as_slice(), &[0xAB; 40]].concat()],
        )
        .unwrap();
        assert_eq!(s.read_session_key(), None);
    }

    #[test]
    fn failed_insert_rolls_back() {
        let (_d, s) = fixture("24");
        s.write_session("old", "o").unwrap();
        let conn = Connection::open(&s.path).unwrap();
        conn.execute("ALTER TABLE cookies DROP COLUMN has_cross_site_ancestor", [])
            .unwrap();
        drop(conn);
        assert!(s.write_session("new", "o").is_err());
        assert_eq!(s.read_session_key().as_deref(), Some("old"));
    }
}
