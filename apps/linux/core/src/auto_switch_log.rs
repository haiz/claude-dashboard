//! The Switch Log: every auto switch, failed attempt and no-candidate stretch in
//! `auto_switch_logs.db`, newest 500 kept. Port of macOS `AutoSwitchLogStore` /
//! `AutoSwitchLogModels`; the schema is the macOS one and raw values never change.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::store::StoreError;

pub const MAX_ENTRIES: i64 = 500;

/// How an evaluation ended. Raw values are stored in `auto_switch_logs.outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Switched = 0,
    Failed = 1,
    /// Logged once per stretch, not on every refresh.
    NoCandidate = 2,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Switched => "Switched",
            Outcome::Failed => "Failed",
            Outcome::NoCandidate => "No candidate",
        }
    }

    fn from_i64(v: i64) -> Option<Self> {
        match v {
            0 => Some(Outcome::Switched),
            1 => Some(Outcome::Failed),
            2 => Some(Outcome::NoCandidate),
            _ => None,
        }
    }
}

/// One row. Account names are copied in when logged, so a row still reads right after the
/// account is renamed or removed.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: i64,
    pub date_unix: f64,
    pub outcome: Outcome,
    pub from_name: Option<String>,
    pub to_name: Option<String>,
    /// Why auto switch acted: the window over its limit and the tier, or the 5h reset.
    pub reason: String,
    /// The target's usage, or the error for a failed switch.
    pub detail: Option<String>,
}

/// A row to insert (no id yet).
#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    pub date_unix: f64,
    pub outcome: Outcome,
    pub from_name: Option<String>,
    pub to_name: Option<String>,
    pub reason: String,
    pub detail: Option<String>,
}

pub struct AutoSwitchLogStore {
    conn: Connection,
    max_entries: i64,
}

impl AutoSwitchLogStore {
    pub fn open() -> Result<Self, StoreError> {
        Self::open_at(&crate::store::auto_switch_log_path(), MAX_ENTRIES)
    }

    pub fn open_at(path: &Path, max_entries: i64) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?, max_entries)
    }

    pub fn open_in_memory(max_entries: i64) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?, max_entries)
    }

    fn from_connection(conn: Connection, max_entries: i64) -> Result<Self, StoreError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS auto_switch_logs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 date REAL NOT NULL,
                 outcome INTEGER NOT NULL,
                 from_name TEXT,
                 to_name TEXT,
                 reason TEXT NOT NULL,
                 detail TEXT)",
        )?;
        Ok(AutoSwitchLogStore { conn, max_entries })
    }

    /// Best-effort: a SQLite failure returns -1 and logs nothing.
    pub fn insert(&self, e: &NewEntry) -> i64 {
        let inserted = self.conn.execute(
            "INSERT INTO auto_switch_logs (date, outcome, from_name, to_name, reason, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![e.date_unix, e.outcome as i64, e.from_name, e.to_name, e.reason, e.detail],
        );
        if inserted.is_err() {
            return -1;
        }
        let id = self.conn.last_insert_rowid();
        let _ = self.conn.execute(
            "DELETE FROM auto_switch_logs WHERE id NOT IN (SELECT id FROM auto_switch_logs ORDER BY id DESC LIMIT ?1)",
            params![self.max_entries],
        );
        id
    }

    /// Newest first.
    pub fn fetch(&self, limit: i64) -> Vec<Entry> {
        let run = || -> rusqlite::Result<Vec<Entry>> {
            let mut stmt = self.conn.prepare(
                "SELECT id, date, outcome, from_name, to_name, reason, detail FROM auto_switch_logs ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, date_unix, outcome, from_name, to_name, reason, detail) = row?;
                let Some(outcome) = Outcome::from_i64(outcome) else { continue };
                out.push(Entry { id, date_unix, outcome, from_name, to_name, reason: reason.unwrap_or_default(), detail });
            }
            Ok(out)
        };
        run().unwrap_or_default()
    }

    pub fn clear(&self) {
        let _ = self.conn.execute("DELETE FROM auto_switch_logs", []);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1_800_000_000.0;

    fn entry(t: f64, outcome: Outcome, from: Option<&str>, to: Option<&str>, reason: &str, detail: Option<&str>) -> NewEntry {
        NewEntry {
            date_unix: t,
            outcome,
            from_name: from.map(Into::into),
            to_name: to.map(Into::into),
            reason: reason.into(),
            detail: detail.map(Into::into),
        }
    }

    #[test]
    fn insert_then_fetch_returns_newest_first_with_every_field() {
        let s = AutoSwitchLogStore::open_in_memory(MAX_ENTRIES).unwrap();
        s.insert(&entry(T0, Outcome::Switched, Some("a@x.com"), Some("b@x.com"), "5h 96% >= 95%, tier 1", Some("b@x.com: 5h 10%, 7d 20%")));
        s.insert(&entry(T0 + 60.5, Outcome::NoCandidate, Some("b@x.com"), None, "7d 99% >= 99%", None));
        let rows = s.fetch(10);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].outcome, Outcome::NoCandidate);
        assert_eq!(rows[0].date_unix, T0 + 60.5, "fractional seconds kept (REAL column)");
        assert_eq!((rows[0].to_name.clone(), rows[0].detail.clone()), (None, None));
        assert_eq!(
            rows[1],
            Entry {
                id: rows[1].id,
                date_unix: T0,
                outcome: Outcome::Switched,
                from_name: Some("a@x.com".into()),
                to_name: Some("b@x.com".into()),
                reason: "5h 96% >= 95%, tier 1".into(),
                detail: Some("b@x.com: 5h 10%, 7d 20%".into()),
            }
        );
    }

    #[test]
    fn keeps_only_the_newest_rows() {
        let s = AutoSwitchLogStore::open_in_memory(2).unwrap();
        for i in 0..3 {
            s.insert(&entry(T0 + i as f64, Outcome::Switched, Some("a"), Some("b"), &format!("r{i}"), None));
        }
        assert_eq!(s.fetch(10).iter().map(|e| e.reason.as_str()).collect::<Vec<_>>(), vec!["r2", "r1"]);
    }

    #[test]
    fn clear_empties_the_log() {
        let s = AutoSwitchLogStore::open_in_memory(MAX_ENTRIES).unwrap();
        s.insert(&entry(T0, Outcome::Failed, Some("a"), Some("b"), "r", Some("e")));
        s.clear();
        assert!(s.fetch(10).is_empty());
    }

    #[test]
    fn outcome_raw_values_are_fixed_and_unknown_rows_skipped() {
        assert_eq!([Outcome::Switched as i64, Outcome::Failed as i64, Outcome::NoCandidate as i64], [0, 1, 2]);
        assert_eq!(Outcome::NoCandidate.label(), "No candidate");
        let s = AutoSwitchLogStore::open_in_memory(MAX_ENTRIES).unwrap();
        s.insert(&entry(T0, Outcome::Failed, None, None, "r", None));
        s.conn
            .execute("INSERT INTO auto_switch_logs (date, outcome, reason) VALUES (?1, 7, 'x')", params![T0])
            .unwrap();
        assert_eq!(s.fetch(10).len(), 1);
    }

    #[test]
    fn on_disk_store_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("auto_switch_logs.db");
        AutoSwitchLogStore::open_at(&p, MAX_ENTRIES).unwrap().insert(&entry(T0, Outcome::Switched, None, None, "r", None));
        assert_eq!(AutoSwitchLogStore::open_at(&p, MAX_ENTRIES).unwrap().fetch(10).len(), 1);
    }
}
