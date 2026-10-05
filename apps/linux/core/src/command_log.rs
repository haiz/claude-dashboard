//! The command run log. Ports `CommandLogStore.swift` / `CommandLogModels.swift`:
//! the trigger/status vocabulary (raw values are persisted, so fixed), newest-
//! first reads, the 500-row cap by id, and the 4096-byte output tail.

use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection};

use crate::store::{command_log_path, StoreError};

pub const MAX_ENTRIES: i64 = 500;
pub const MAX_OUTPUT_BYTES: usize = 4096;

/// The last `max_bytes` bytes of `s`, moved forward to a char boundary so a
/// multibyte char at the cut is dropped whole rather than split.
pub fn bounded_tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTrigger {
    Manual = 0,
    AutoReset = 1,
    AutoEmpty = 2,
}

impl CommandTrigger {
    pub fn from_raw(v: i64) -> Self {
        match v {
            1 => CommandTrigger::AutoReset,
            2 => CommandTrigger::AutoEmpty,
            _ => CommandTrigger::Manual,
        }
    }
    pub fn raw(self) -> i64 {
        self as i64
    }
    pub fn label(self) -> &'static str {
        match self {
            CommandTrigger::Manual => "Manual",
            CommandTrigger::AutoReset => "Auto (reset)",
            CommandTrigger::AutoEmpty => "Auto (empty)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Exited = 0,
    TimedOut = 1,
    Cancelled = 2,
    LaunchedInTerminal = 3,
    LaunchFailed = 4,
}

impl CommandStatus {
    /// NULL (rows from before the column existed) and unknown values read as `Exited`.
    pub fn from_raw(v: Option<i64>) -> Self {
        match v {
            Some(1) => CommandStatus::TimedOut,
            Some(2) => CommandStatus::Cancelled,
            Some(3) => CommandStatus::LaunchedInTerminal,
            Some(4) => CommandStatus::LaunchFailed,
            _ => CommandStatus::Exited,
        }
    }
    pub fn raw(self) -> i64 {
        self as i64
    }
    pub fn label(self) -> &'static str {
        match self {
            CommandStatus::Exited => "Exited",
            CommandStatus::TimedOut => "Timed out",
            CommandStatus::Cancelled => "Cancelled",
            CommandStatus::LaunchedInTerminal => "In Terminal",
            CommandStatus::LaunchFailed => "Launch failed",
        }
    }
}

pub struct NewEntry<'a> {
    pub account_id: Option<&'a str>,
    pub command: &'a str,
    pub trigger: CommandTrigger,
    pub started_unix: i64,
    pub finished_unix: Option<i64>,
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub output: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandLogEntry {
    pub id: i64,
    pub account_id: Option<String>,
    pub command: String,
    pub trigger: CommandTrigger,
    pub started_unix: i64,
    pub finished_unix: Option<i64>,
    pub exit_code: Option<i32>,
    pub status: CommandStatus,
    pub output: Option<String>,
}

const SCHEMA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS command_logs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        account_id TEXT,
        cmd TEXT NOT NULL,
        trig INTEGER NOT NULL,
        started INTEGER NOT NULL,
        finished INTEGER,
        exit_code INTEGER,
        status INTEGER,
        output TEXT
    );
";

/// One connection per store. Several app threads may each open their own
/// (a manual run and an auto run can finish together); the busy timeout lets
/// SQLite serialise them instead of failing with SQLITE_BUSY.
pub struct CommandLogStore {
    conn: Connection,
    max_entries: i64,
}

impl CommandLogStore {
    pub fn open() -> Result<Self, StoreError> {
        Self::open_at(&command_log_path())
    }

    pub fn open_at(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn, max_entries: MAX_ENTRIES })
    }

    pub fn with_max_entries(mut self, n: i64) -> Self {
        self.max_entries = n;
        self
    }

    pub fn record(&self, e: &NewEntry) -> Result<i64, StoreError> {
        let output = e
            .output
            .filter(|o| !o.is_empty())
            .map(|o| bounded_tail(o, MAX_OUTPUT_BYTES));
        self.conn.execute(
            "INSERT INTO command_logs (account_id, cmd, trig, started, finished, exit_code, status, output) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                e.account_id,
                e.command,
                e.trigger.raw(),
                e.started_unix,
                e.finished_unix,
                e.exit_code,
                e.status.raw(),
                output
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        // Cap by id, not time: a row recorded out of clock order keeps its place.
        self.conn.execute(
            "DELETE FROM command_logs WHERE id NOT IN \
             (SELECT id FROM command_logs ORDER BY id DESC LIMIT ?1)",
            params![self.max_entries],
        )?;
        Ok(id)
    }

    /// Newest first. A read failure yields an empty list (the log is advisory).
    pub fn recent(&self, limit: usize) -> Vec<CommandLogEntry> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT id, account_id, cmd, trig, started, finished, exit_code, status, output \
             FROM command_logs ORDER BY id DESC LIMIT ?1",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(CommandLogEntry {
                id: r.get(0)?,
                account_id: r.get(1)?,
                command: r.get(2)?,
                trigger: CommandTrigger::from_raw(r.get(3)?),
                started_unix: r.get(4)?,
                finished_unix: r.get(5)?,
                exit_code: r.get(6)?,
                status: CommandStatus::from_raw(r.get(7)?),
                output: r.get(8)?,
            })
        });
        match rows {
            Ok(it) => it.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn clear(&self) -> Result<usize, StoreError> {
        Ok(self.conn.execute("DELETE FROM command_logs", [])?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(cmd: &'a str, started: i64) -> NewEntry<'a> {
        NewEntry {
            account_id: Some("acct-1"),
            command: cmd,
            trigger: CommandTrigger::Manual,
            started_unix: started,
            finished_unix: Some(started + 2),
            status: CommandStatus::Exited,
            exit_code: Some(0),
            output: Some("ok"),
        }
    }

    #[test]
    fn vocabulary_raw_values_and_labels_are_fixed() {
        assert_eq!(CommandTrigger::Manual.raw(), 0);
        assert_eq!(CommandTrigger::AutoReset.raw(), 1);
        assert_eq!(CommandTrigger::AutoEmpty.raw(), 2);
        assert_eq!(CommandTrigger::AutoEmpty.label(), "Auto (empty)");
        assert_eq!(CommandTrigger::AutoReset.label(), "Auto (reset)");
        assert_eq!(CommandTrigger::from_raw(99), CommandTrigger::Manual);
        assert_eq!(CommandStatus::LaunchFailed.raw(), 4);
        assert_eq!(CommandStatus::LaunchedInTerminal.label(), "In Terminal");
        assert_eq!(CommandStatus::TimedOut.label(), "Timed out");
        assert_eq!(CommandStatus::from_raw(None), CommandStatus::Exited);
        assert_eq!(CommandStatus::from_raw(Some(42)), CommandStatus::Exited);
    }

    #[test]
    fn record_then_recent_roundtrips_newest_first() {
        let s = CommandLogStore::open_in_memory().unwrap();
        let a = s.record(&entry("first", 100)).unwrap();
        let b = s.record(&entry("second", 50)).unwrap(); // older clock, newer id
        assert!(b > a);
        let rows = s.recent(10);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].command, "second");
        assert_eq!(rows[1].command, "first");
        assert_eq!(rows[1].account_id.as_deref(), Some("acct-1"));
        assert_eq!(rows[1].finished_unix, Some(102));
        assert_eq!(rows[1].exit_code, Some(0));
        assert_eq!(rows[1].output.as_deref(), Some("ok"));
    }

    #[test]
    fn nulls_roundtrip() {
        let s = CommandLogStore::open_in_memory().unwrap();
        s.record(&NewEntry {
            account_id: None,
            command: "broken",
            trigger: CommandTrigger::AutoEmpty,
            started_unix: 1,
            finished_unix: None,
            status: CommandStatus::LaunchFailed,
            exit_code: None,
            output: None,
        })
        .unwrap();
        let r = &s.recent(1)[0];
        assert_eq!(r.account_id, None);
        assert_eq!(r.trigger, CommandTrigger::AutoEmpty);
        assert_eq!(r.finished_unix, None);
        assert_eq!(r.exit_code, None);
        assert_eq!(r.status, CommandStatus::LaunchFailed);
        assert_eq!(r.output, None);
    }

    #[test]
    fn empty_output_is_stored_as_null() {
        let s = CommandLogStore::open_in_memory().unwrap();
        let mut e = entry("x", 1);
        e.output = Some("");
        s.record(&e).unwrap();
        assert_eq!(s.recent(1)[0].output, None);
    }

    #[test]
    fn cap_keeps_newest_by_id() {
        let s = CommandLogStore::open_in_memory().unwrap().with_max_entries(3);
        for i in 0..5 {
            s.record(&entry(&format!("c{i}"), 1000 - i)).unwrap();
        }
        let cmds: Vec<String> = s.recent(10).into_iter().map(|r| r.command).collect();
        assert_eq!(cmds, vec!["c4", "c3", "c2"]);
    }

    #[test]
    fn default_cap_is_500() {
        assert_eq!(MAX_ENTRIES, 500);
        assert_eq!(MAX_OUTPUT_BYTES, 4096);
    }

    #[test]
    fn recorded_output_is_bounded() {
        let s = CommandLogStore::open_in_memory().unwrap();
        let big = "a".repeat(10_000) + "END";
        let mut e = entry("x", 1);
        e.output = Some(&big);
        s.record(&e).unwrap();
        let out = s.recent(1)[0].output.clone().unwrap();
        assert_eq!(out.len(), MAX_OUTPUT_BYTES);
        assert!(out.ends_with("END"));
    }

    #[test]
    fn bounded_tail_keeps_short_text() {
        assert_eq!(bounded_tail("hello", 10), "hello");
        assert_eq!(bounded_tail("", 10), "");
    }

    #[test]
    fn bounded_tail_never_splits_a_char() {
        // "é" is 2 bytes; a 3-byte cap over "éé" must not start mid-char.
        let t = bounded_tail("éé", 3);
        assert_eq!(t, "é");
        let t = bounded_tail("abc日本", 4); // 日本 = 6 bytes
        assert_eq!(t, "本");
    }

    #[test]
    fn clear_removes_all_and_reports_count() {
        let s = CommandLogStore::open_in_memory().unwrap();
        s.record(&entry("a", 1)).unwrap();
        s.record(&entry("b", 2)).unwrap();
        assert_eq!(s.clear().unwrap(), 2);
        assert!(s.recent(10).is_empty());
    }

    #[test]
    fn open_at_creates_parent_dir_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested").join("command_logs.db");
        CommandLogStore::open_at(&p).unwrap().record(&entry("kept", 1)).unwrap();
        let again = CommandLogStore::open_at(&p).unwrap();
        assert_eq!(again.recent(1)[0].command, "kept");
    }

    #[test]
    fn path_is_command_logs_db() {
        assert_eq!(crate::store::command_log_path().file_name().unwrap(), "command_logs.db");
    }
}
