//! The Ping Log: one SQLite row per finished ping, newest 500 kept. Port of macOS
//! `PingLogStore` / `PingLogModels`; the schema is the macOS one and raw values never change.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::ping::{PingOutcome, PingResult};
use crate::store::StoreError;

/// What started a ping. Raw values are stored in `ping_logs.trig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingTrigger {
    Manual = 0,
    Auto = 1,
}

impl PingTrigger {
    pub fn label(self) -> &'static str {
        match self {
            PingTrigger::Manual => "Manual",
            PingTrigger::Auto => "Auto",
        }
    }

    fn from_i64(v: i64) -> Self {
        if v == 1 { PingTrigger::Auto } else { PingTrigger::Manual }
    }
}

/// One finished ping, as the log stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct PingLogEntry {
    pub id: i64,
    pub account_id: String,
    pub trigger: PingTrigger,
    pub started_unix: f64,
    pub finished_unix: f64,
    pub outcome: PingOutcome,
    pub detail: Option<String>,
}

impl PingLogEntry {
    /// The line under an account page's Ping button; `time` is the finish time, formatted.
    pub fn caption(&self, time: &str) -> String {
        if self.outcome == PingOutcome::Ok {
            return match &self.detail {
                Some(d) => format!("Pinged at {time} ({d})"),
                None => format!("Pinged at {time}"),
            };
        }
        let reason = match &self.detail {
            Some(d) => format!("{}, {d}", self.outcome.label()),
            None => self.outcome.label().to_string(),
        };
        format!("Ping failed at {time}: {reason}")
    }
}

pub const MAX_ENTRIES: usize = 500;

const SCHEMA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS ping_logs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        account_id TEXT NOT NULL,
        trig INTEGER NOT NULL,
        started REAL NOT NULL,
        finished REAL NOT NULL,
        outcome INTEGER NOT NULL,
        detail TEXT
    );
";

const COLUMNS: &str = "id, account_id, trig, started, finished, outcome, detail";

fn entry_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<PingLogEntry> {
    Ok(PingLogEntry {
        id: row.get(0)?,
        account_id: row.get(1)?,
        trigger: PingTrigger::from_i64(row.get(2)?),
        started_unix: row.get(3)?,
        finished_unix: row.get(4)?,
        outcome: PingOutcome::from_i64(row.get(5)?).unwrap_or(PingOutcome::Failed),
        detail: row.get(6)?,
    })
}

pub struct PingLogStore {
    conn: Connection,
    max_entries: usize,
}

impl PingLogStore {
    pub fn open() -> Result<Self, StoreError> {
        Self::open_at(&crate::store::ping_log_path(), MAX_ENTRIES)
    }

    pub fn open_at(path: &Path, max_entries: usize) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?, max_entries)
    }

    pub fn open_in_memory(max_entries: usize) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?, max_entries)
    }

    fn from_connection(conn: Connection, max_entries: usize) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn, max_entries })
    }

    /// Writes one finished ping and keeps the newest `max_entries`. Returns its row id
    /// (0 when SQLite failed: the log is best effort, like the usage log).
    pub fn insert(&mut self, account_id: &str, trigger: PingTrigger, started: f64, finished: f64, result: &PingResult) -> i64 {
        let inserted = self.conn.execute(
            "INSERT INTO ping_logs (account_id, trig, started, finished, outcome, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![account_id, trigger as i64, started, finished, result.outcome as i64, result.detail],
        );
        if inserted.is_err() {
            return 0;
        }
        let id = self.conn.last_insert_rowid();
        let _ = self.conn.execute(
            "DELETE FROM ping_logs WHERE id NOT IN (SELECT id FROM ping_logs ORDER BY id DESC LIMIT ?1)",
            params![self.max_entries as i64],
        );
        id
    }

    /// Newest first.
    pub fn fetch(&self, limit: usize) -> Vec<PingLogEntry> {
        let sql = format!("SELECT {COLUMNS} FROM ping_logs ORDER BY id DESC LIMIT ?1");
        let Ok(mut stmt) = self.conn.prepare(&sql) else { return Vec::new() };
        stmt.query_map(params![limit as i64], entry_from)
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    pub fn latest(&self, account_id: &str) -> Option<PingLogEntry> {
        let sql = format!("SELECT {COLUMNS} FROM ping_logs WHERE account_id = ?1 ORDER BY id DESC LIMIT 1");
        self.conn.query_row(&sql, params![account_id], entry_from).optional().ok().flatten()
    }

    /// Every account's newest row (the account pages' captions at launch).
    pub fn latest_all(&self) -> HashMap<String, PingLogEntry> {
        let mut out = HashMap::new();
        for e in self.fetch(self.max_entries).into_iter().rev() {
            out.insert(e.account_id.clone(), e);
        }
        out
    }

    pub fn clear(&mut self) {
        let _ = self.conn.execute("DELETE FROM ping_logs", []);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> PingResult {
        PingResult { outcome: PingOutcome::Ok, detail: None }
    }

    fn entry(outcome: PingOutcome, detail: Option<&str>) -> PingLogEntry {
        PingLogEntry {
            id: 1,
            account_id: "a".into(),
            trigger: PingTrigger::Manual,
            started_unix: 1.0,
            finished_unix: 2.0,
            outcome,
            detail: detail.map(String::from),
        }
    }

    #[test]
    fn insert_then_fetch_newest_first() {
        let mut s = PingLogStore::open_in_memory(500).unwrap();
        s.insert("a", PingTrigger::Manual, 10.0, 11.5, &ok());
        let failed = PingResult { outcome: PingOutcome::Failed, detail: Some("HTTP 429".into()) };
        let id = s.insert("b", PingTrigger::Auto, 20.0, 21.0, &failed);
        let rows = s.fetch(10);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].account_id, "b");
        assert_eq!(rows[0].trigger, PingTrigger::Auto);
        assert_eq!(rows[0].outcome, PingOutcome::Failed);
        assert_eq!(rows[0].detail.as_deref(), Some("HTTP 429"));
        assert_eq!((rows[1].started_unix, rows[1].finished_unix), (10.0, 11.5));
        assert_eq!(rows[1].trigger.label(), "Manual");
    }

    #[test]
    fn latest_per_account() {
        let mut s = PingLogStore::open_in_memory(500).unwrap();
        s.insert("a", PingTrigger::Manual, 1.0, 2.0, &ok());
        s.insert("b", PingTrigger::Auto, 3.0, 4.0, &ok());
        s.insert("a", PingTrigger::Auto, 5.0, 6.0, &ok());
        assert_eq!(s.latest("a").unwrap().started_unix, 5.0);
        assert!(s.latest("z").is_none());
        let all = s.latest_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all["a"].started_unix, 5.0);
        assert_eq!(all["b"].started_unix, 3.0);
    }

    #[test]
    fn prunes_to_max_entries() {
        let mut s = PingLogStore::open_in_memory(3).unwrap();
        for i in 0..5 {
            s.insert("a", PingTrigger::Auto, i as f64, i as f64, &ok());
        }
        let starts: Vec<f64> = s.fetch(10).iter().map(|e| e.started_unix).collect();
        assert_eq!(starts, vec![4.0, 3.0, 2.0]);
    }

    #[test]
    fn clear_empties() {
        let mut s = PingLogStore::open_in_memory(500).unwrap();
        s.insert("a", PingTrigger::Manual, 1.0, 2.0, &ok());
        s.clear();
        assert!(s.fetch(10).is_empty());
    }

    #[test]
    fn file_store_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ping_logs.db");
        PingLogStore::open_at(&p, 500).unwrap().insert("a", PingTrigger::Manual, 1.0, 2.0, &ok());
        assert_eq!(PingLogStore::open_at(&p, 500).unwrap().fetch(10).len(), 1);
    }

    #[test]
    fn caption_texts() {
        let t = "10:44 AM";
        assert_eq!(entry(PingOutcome::Ok, None).caption(t), "Pinged at 10:44 AM");
        assert_eq!(
            entry(PingOutcome::Ok, Some("conversation not deleted")).caption(t),
            "Pinged at 10:44 AM (conversation not deleted)"
        );
        assert_eq!(entry(PingOutcome::Expired, None).caption(t), "Ping failed at 10:44 AM: Session expired");
        assert_eq!(entry(PingOutcome::Failed, Some("HTTP 429")).caption(t), "Ping failed at 10:44 AM: Failed, HTTP 429");
    }

    #[test]
    fn path_is_beside_usage_log() {
        let p = crate::store::ping_log_path();
        assert_eq!(p.file_name().unwrap(), "ping_logs.db");
        assert_eq!(p.parent(), crate::store::usage_log_path().parent());
    }
}
