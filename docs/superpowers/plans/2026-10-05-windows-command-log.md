# Windows Sub-project 5: Command Log — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Per-account saved run commands in the Windows app. A command can be run by hand (Run Command panel) or fire on its own when a usage window resets. Non-interactive runs are hidden and go through a Job Object, with timeout, cancel and a bounded output tail. Interactive runs open in Windows Terminal. Every run lands in a Command Log pane backed by SQLite. The Settings shell picker, the active-Claude-Code badge and sort tier, and the Help pane are part of this sub-project too.

**Architecture:** Pure, cross-platform rules go in `core` (`apps/linux/core`), TDD'd, and also run on the Linux CI job:
- the command-log SQLite store and its vocabulary
- the interactive/non-interactive classifier
- the auto-run latch
- the `~/.claude.json` reader and the tier-2 sort
- the per-account saved-command store

Windows process control lives in the app crate (`apps/windows/app`): shell discovery and argv building, a Job Object runner, the terminal launcher, and glue that records each run. These are tested against real `cmd.exe` children. The Slint panes call the glue from worker threads and get results back via `slint::invoke_from_event_loop`.

**Tech Stack:** Rust 1.98.0 (pinned), `rusqlite` (already a `core` dep), `windows-sys` 0.59 (Job Objects, ToolHelp), Slint 1.

**Spec:** `docs/superpowers/specs/2026-10-04-windows-app-design.md` — section "Command Log" (sub-project 5 of 6) and the "Help" line under it. Resume context: `docs/superpowers/HANDOFF-windows-port.md` §4–§5.

**Port sources (read for behaviour, never copy Swift idioms blindly):**
- `apps/macos/ClaudeDashboard/Services/{CommandRunner,CommandClassifier,TerminalLauncher,CommandLogStore,ClaudeCodeAccountDetector}.swift`
- `apps/macos/ClaudeDashboard/Models/{CommandLogModels,RunCommandSettings}.swift`
- `apps/macos/ClaudeDashboard/Views/{CommandLogView,RunCommandSheet,HelpView}.swift`
- `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift:208-211,278-293,345-361,559-562,701-715`
- The Linux pure ports, which already resolved several Swift ambiguities: `apps/linux/lib/{commandLog,commandClassifier,autoRun,claudeCode}.js`

**Prior sub-projects (reuse, don't rebuild):** `core::store` (`data_dir`, `accounts_path`, `StoreError`, `lock_store`), `core::settings` (`settings.json`, load-never-fails), `core::rows` (`DisplayRow`, `WindowView`, `build_rows`), app `refresh.rs` / `main.rs::spawn_refresh_loop`, `settings_accounts::delete_account`, `model::{to_ui_row, local_offset_s}`, `testenv::lock()`, the `ChartPanel` overlay pattern in `app.slint`.

## Global Constraints

- Toolchain 1.98.0 pinned; workspaces declare `rust-version = 1.89`. `core` must still build and test on Linux (`cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` in `apps/linux`). Use `cfg(windows)` only for Windows paths.
- `apps/windows`: `cargo test --workspace` and `cargo clippy --all-targets -- -D warnings` must pass on every task.
- Persisted raw values are fixed (`CommandLogModels.swift`): trigger `manual=0, autoReset=1, autoEmpty=2`; status `exited=0, timedOut=1, cancelled=2, launchedInTerminal=3, launchFailed=4`. Labels: `Manual`, `Auto (reset)`, `Auto (empty)`; `Exited`, `Timed out`, `Cancelled`, `In Terminal`, `Launch failed`.
- Command log: keep the newest **500** rows by id. Output tail is at most **4096 bytes** (UTF-8, cut on a char boundary). An empty output is stored as NULL. Timestamps are Unix **seconds** (`i64`).
- Run timeout **60 s** (`CommandRunner.timeout`). Resolver timeout **5 s**. Classifier debounce **350 ms**.
- Data paths: `%LOCALAPPDATA%\claude-dashboard\command_logs.db`; `%APPDATA%\claude-dashboard\run-commands.json`; the shell choice is `settings.json` key `"shell"` (`"pwsh" | "powershell" | "cmd" | "bash"`, absent = auto).
- Hidden runs: stdin is NUL, `CREATE_NO_WINDOW`, cwd = `%USERPROFILE%`. Every child runs inside a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assigned **before** its first instruction runs (spawn suspended → assign → resume).
- The session key never appears in a command, log row, UI string or error.
- GUI work runs off the UI thread (`std::thread::spawn` + `slint::invoke_from_event_loop`). Timers stop on close, so the `CLAUDE_DASHBOARD_SMOKE=1` self-quit still fires.
- Edit `.slint`, `.rs` and `.md` files with the Edit tool. Never use perl, sed or `Set-Content` (silent misses, UTF-8 BOM).
- Commit author must be explicit: `git -c user.email="backend@gotitapp.co" -c user.name="cthai" commit ...`. End every message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Rulings (decided here so implementers don't re-litigate)

1. **`autoEmpty` is vocabulary only.** macOS defines it but never fires it. The only auto rule is `shouldRunSavedCommand`, which checks for a window missing `resets_at`, and that rule fires `autoReset`. Windows keeps the raw value and the label for log compatibility and fires only `autoReset`. *Cost if wrong:* adding an "empty" rule later is one predicate in `core::auto_run`.
2. **Auto runs are always hidden (non-interactive)**, even when the saved toggle says "Open in Terminal". This matches macOS `refreshAll`, which calls `runner.run`. With stdin on NUL and the 60 s timeout, a TUI command cannot hang the app.
3. **Background children die with the shell.** When the shell process exits, the runner closes the job, and `KILL_ON_JOB_CLOSE` ends anything the command left running in the background. Without that, a lingering grandchild would hold the output pipe open and the run would never finish. This is documented in `contract/windows.md`. macOS kills the tree only on timeout or cancel; here the job makes it uniform.
4. **Saved commands are keyed by account id and dropped when the account is deleted.** This follows `RunCommandSettings.remove`. A re-sync or key repair keeps the id, so the command survives it. Delete and re-add creates a new id, so the command is gone by construction. The macOS `prune` covers stores from older versions that never cleaned up; Windows has no such history, so there is no prune.
5. **The auto-run latch only arms for accounts that have a saved command.** This matches macOS, which inserts into `pingedAccounts` only when a command exists. A command saved mid-episode therefore fires on the next refresh. This deliberately differs from the Linux `AutoRunLatch`, which arms regardless.
6. **GUI tasks (9, 10, 11, 12) are verified by build + run-check + the named pure helpers only.** This is the same ruling as SP2–SP4.

## Review Focus

1. **Cancel or timeout must kill the whole tree, and a normal exit must not hang on a lingering background child.** `start /b ping …` exits its shell immediately, but the ping still holds the pipe; the run must still return within seconds. → Task 7 tests `timeout_kills_tree`, `cancel_kills_tree`, `background_grandchild_does_not_hang`.
2. **An interactive program run hidden must not hang.** Covers an auto-run of `claude`, or a classifier miss. A child that reads stdin gets EOF immediately; a TUI that ignores EOF hits the timeout. → Task 7 test `stdin_is_null_so_reads_return`.
3. **Auto-run fires once per reset episode.** It re-arms only when both windows report a reset time again, never fires for an account with no (or blank) saved command, and does fire for a command saved mid-episode. → Task 4 tests.
4. **Output stays bounded.** A chatty command keeps at most 4096 bytes and ends with its last line. A multibyte char at the cut is never split. → Task 1 `bounded_tail_*` tests + Task 7 `chatty_output_is_bounded`.
5. **A leading token with shell metacharacters never reaches the resolve script** (`$(…)`, backtick, `;`, `|`, `&`, quotes, `%`). The resolver also gives up after 5 s on a hung profile. → Task 2 `resolvable_token_*` tests + Task 8 `classify_uses_first_token_when_resolution_fails`.

## File structure

```
apps/linux/core/src/
  command_log.rs       (new) CommandTrigger, CommandStatus, NewEntry, CommandLogEntry,
                             CommandLogStore (SQLite), bounded_tail, MAX_ENTRIES, MAX_OUTPUT_BYTES
  command_classifier.rs(new) CommandKind, classify(command, expanded), strip_quoted_regions,
                             ssh_interactive, leading_token, resolvable_token
  auto_run.rs          (new) should_run_saved_command(&DisplayRow), AutoRunLatch
  claude_code.rs       (new) active_email_from(text), claude_json_path(), active_email()
  run_commands.rs      (new) RunCommand, run_commands_path(), get/set/remove (+ *_at for tests)
  rows.rs              (mod) BuildInput.active_claude_code_email, DisplayRow.is_active_claude_code, tier-2 sort
  settings.rs          (mod) Settings.shell; pub(crate) write_atomic shared with run_commands
  store.rs             (mod) command_log_path()
  lib.rs               (mod) pub mod lines
apps/windows/app/
  Cargo.toml           (mod) windows-sys features Win32_System_JobObjects, Win32_System_Diagnostics_ToolHelp
  src/shell.rs         (new) ShellKind, ShellSpec, Invocation, Probe, locate/available/resolve/detect,
                             run_invocation, resolve_invocation
  src/terminal.rs      (new) terminal_invocation (wt.exe / conhost), wt_escape, launch
  src/runner.rs        (new) CancelToken, RunOutcome, run(&Invocation, timeout, &CancelToken, on_output) — Job Object
  src/commands.rs      (new) execute / launch_in_terminal (record to the log), classify (resolver)
  src/log_view.rs      (new) CommandLogEntry -> UiLogEntry pure mapping + load/reload/clear glue
  src/run_command.rs   (new) Run Command panel controller (debounced classify, run, cancel)
  src/main.rs          (mod) mods, auto-run in the refresh loop, installs, fake rows badge
  src/refresh.rs       (mod) pass active Claude Code email into build_rows
  src/model.rs         (mod) UiRow.claude-code
  src/settings_accounts.rs (mod) delete also removes the saved command
  src/settings_general.rs  (mod) shell list / current / set
  ui/command_log.slint (new) UiLogEntry, CommandLogPane
  ui/run_command.slint (new) RunCommandPanel
  ui/help.slint        (new) HelpPane
  ui/theme.slint       (mod) UiRow.claude-code
  ui/components.slint  (mod) Claude Code dot on card + pane, card run button, pane "Run Command" button
  ui/app.slint         (mod) Command Log + Help sidebar rows, panes, run overlay, shell chips
contract/windows.md    (mod) paths, Command Log section
CLAUDE.md              (mod) SP5 modules
```

Task order is dependency order. Tasks 1–5 are `core` (TDD), 6–8 are app process control (TDD against `cmd.exe`), and 9–12 are GUI (build + run-check + named pure helpers). Task 13 is docs.

---

### Task 1: `core::command_log` — store and vocabulary (TDD)

**Files:**
- Create: `apps/linux/core/src/command_log.rs`
- Modify: `apps/linux/core/src/store.rs` (add `command_log_path` next to `usage_log_path`, ~line 141)
- Modify: `apps/linux/core/src/lib.rs` (add `pub mod command_log;` among the ungated mods)

**Interfaces:**
- Consumes: `crate::store::{StoreError, data_dir}` (`data_dir` is private in store.rs; the new `command_log_path` lives in store.rs so it can call it).
- Produces:
  - `pub fn store::command_log_path() -> PathBuf` → `<data dir>/claude-dashboard/command_logs.db`
  - `pub const MAX_ENTRIES: i64 = 500; pub const MAX_OUTPUT_BYTES: usize = 4096;`
  - `pub fn bounded_tail(s: &str, max_bytes: usize) -> &str`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum CommandTrigger { Manual = 0, AutoReset = 1, AutoEmpty = 2 }` with `from_raw(i64) -> Self` (unknown → `Manual`), `raw(self) -> i64`, `label(self) -> &'static str`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum CommandStatus { Exited = 0, TimedOut = 1, Cancelled = 2, LaunchedInTerminal = 3, LaunchFailed = 4 }` with `from_raw(Option<i64>) -> Self` (NULL/unknown → `Exited`), `raw`, `label`
  - `pub struct NewEntry<'a> { pub account_id: Option<&'a str>, pub command: &'a str, pub trigger: CommandTrigger, pub started_unix: i64, pub finished_unix: Option<i64>, pub status: CommandStatus, pub exit_code: Option<i32>, pub output: Option<&'a str> }`
  - `#[derive(Debug, Clone, PartialEq)] pub struct CommandLogEntry { pub id: i64, pub account_id: Option<String>, pub command: String, pub trigger: CommandTrigger, pub started_unix: i64, pub finished_unix: Option<i64>, pub exit_code: Option<i32>, pub status: CommandStatus, pub output: Option<String> }`
  - `pub struct CommandLogStore` with `open() -> Result<Self, StoreError>`, `open_at(&Path) -> Result<Self, StoreError>`, `open_in_memory() -> Result<Self, StoreError>`, `with_max_entries(self, i64) -> Self`, `record(&self, &NewEntry) -> Result<i64, StoreError>`, `recent(&self, limit: usize) -> Vec<CommandLogEntry>`, `clear(&self) -> Result<usize, StoreError>`

- [ ] **Step 1: Add the path helper** in `store.rs` directly below `usage_log_path`:

```rust
/// `<data dir>/claude-dashboard/command_logs.db` — the run log, a separate
/// file from the usage log (no shared schema, as on macOS).
pub fn command_log_path() -> PathBuf {
    data_dir().join("claude-dashboard").join("command_logs.db")
}
```

- [ ] **Step 2: Write the failing tests.** Create `command_log.rs` with only the module doc, the `use` lines and this test module (the types don't exist yet):

```rust
//! The command run log. Ports `CommandLogStore.swift` / `CommandLogModels.swift`:
//! the trigger/status vocabulary (raw values are persisted, so fixed), newest-
//! first reads, the 500-row cap by id, and the 4096-byte output tail.

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
```

- [ ] **Step 3: Run to see it fail.** Run `cd apps/linux && cargo test -p claude-dashboard-core command_log`. Expected: compile errors (`NewEntry`, `CommandLogStore` … not found).

- [ ] **Step 4: Implement.** Put this above the test module:

```rust
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
```

If `StoreError` lacks a `From<rusqlite::Error>` or `From<std::io::Error>` impl that the `?` needs, add the missing one in `store.rs` next to the existing impls (`UsageLogStore::open` already relies on both, so they should exist).

- [ ] **Step 5: Run the tests.** Run `cd apps/linux && cargo test -p claude-dashboard-core command_log`. Expected: all 12 pass. Then `cargo clippy --workspace --all-targets -- -D warnings`: clean.

- [ ] **Step 6: Commit.**

```bash
git add apps/linux/core/src/command_log.rs apps/linux/core/src/store.rs apps/linux/core/src/lib.rs
git -c user.email="backend@gotitapp.co" -c user.name="cthai" commit -m "feat(core): command log store and vocabulary

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `core::command_classifier` — pure classification (TDD)

**Files:**
- Create: `apps/linux/core/src/command_classifier.rs`
- Modify: `apps/linux/core/src/lib.rs` (`pub mod command_classifier;`)

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum CommandKind { Interactive, NonInteractive }`
  - `pub fn classify(command: &str, expanded: &str) -> CommandKind`
  - `pub fn leading_token(command: &str) -> &str` — first space-separated token (the whole string if none)
  - `pub fn resolvable_token(command: &str) -> Option<&str>` — the leading token if it matches `^[A-Za-z0-9._/\\:-]+$`, else `None`
  - `pub fn strip_quoted_regions(s: &str) -> String`
  - `pub fn ssh_interactive(command: &str) -> bool`

There is no regex crate in `core`, so `\b` is implemented by hand. A word char is ASCII alphanumeric or `_`. A match of `w` at byte `i` counts only if the char before `i` and the char after `i + w.len()` are both non-word (or the string edge).

- [ ] **Step 1: Write the failing tests** (in the new file, under the doc comment):

```rust
//! Ports `CommandClassifier.swift`'s pure half (and the Linux port's refined
//! ssh walk, `apps/linux/lib/commandClassifier.js`). The caller expands the
//! leading token through the user's shell first (`expanded`); this only reads
//! the result. The verdict is a default — the user's toggle wins.

#[cfg(test)]
mod tests {
    use super::*;
    use CommandKind::*;

    #[test]
    fn plain_commands_are_non_interactive() {
        assert_eq!(classify("ls -la", ""), NonInteractive);
        assert_eq!(classify("git status", "C:\\Program Files\\Git\\cmd\\git.exe"), NonInteractive);
    }

    #[test]
    fn tuis_are_interactive() {
        for c in ["vim notes.md", "nvim", "htop", "less log.txt", "lazygit", "fzf"] {
            assert_eq!(classify(c, ""), Interactive, "{c}");
        }
    }

    #[test]
    fn word_boundaries_hold() {
        // `vi` must not match `video`; `more` must not match `moreutils`.
        assert_eq!(classify("video-convert in.mp4", ""), NonInteractive);
        assert_eq!(classify("moreutils-thing", ""), NonInteractive);
        // Windows paths and extensions are boundaries.
        assert_eq!(classify("C:\\tools\\htop.exe", ""), Interactive);
        assert_eq!(classify("claude.exe", ""), Interactive);
    }

    #[test]
    fn argument_prose_is_ignored() {
        assert_eq!(classify("echo \"no more files\"", ""), NonInteractive);
        assert_eq!(classify("echo vim", ""), NonInteractive);
    }

    #[test]
    fn claude_is_interactive_unless_print_mode() {
        assert_eq!(classify("claude", ""), Interactive);
        assert_eq!(classify("claude -p \"hi\"", ""), NonInteractive);
        assert_eq!(classify("claude --print hi", ""), NonInteractive);
        assert_eq!(classify("claude --output-format=json -x", ""), NonInteractive);
        assert_eq!(classify("claude \"explain the -p flag\"", ""), Interactive);
    }

    #[test]
    fn expansion_reveals_the_real_binary() {
        let body = "ccbf\n{\n    claude --dangerously-skip-permissions\n}";
        assert_eq!(classify("ccbf", body), Interactive);
        let printing = "function ccp { claude --print $args }";
        assert_eq!(classify("ccp hello", printing), NonInteractive);
    }

    #[test]
    fn ssh_without_remote_command_is_interactive() {
        assert_eq!(classify("ssh host", ""), Interactive);
        assert_eq!(classify("ssh -p 22 host", ""), Interactive);
        assert_eq!(classify("ssh -i key.pem user@host", ""), Interactive);
        assert_eq!(classify("ssh host uptime", ""), NonInteractive);
        assert_eq!(classify("ssh -p 22 host \"df -h\"", ""), NonInteractive);
    }

    #[test]
    fn strip_quoted_regions_replaces_each_span_with_one_space() {
        assert_eq!(strip_quoted_regions("a \"b c\" d"), "a   d");
        assert_eq!(strip_quoted_regions("x 'y' z"), "x   z");
        assert_eq!(strip_quoted_regions("open \"unterminated"), "open  ");
    }

    #[test]
    fn leading_token_is_first_space_token() {
        assert_eq!(leading_token("git status"), "git");
        assert_eq!(leading_token("ccbf"), "ccbf");
        assert_eq!(leading_token(""), "");
    }

    #[test]
    fn resolvable_token_accepts_names_and_paths() {
        assert_eq!(resolvable_token("ccbf --x"), Some("ccbf"));
        assert_eq!(resolvable_token("C:\\tools\\a-b_c.exe arg"), Some("C:\\tools\\a-b_c.exe"));
        assert_eq!(resolvable_token("./run.sh"), Some("./run.sh"));
    }

    #[test]
    fn resolvable_token_refuses_metacharacters() {
        for c in ["$(rm -rf ~)", "`id`", "a;b", "a|b", "a&b", "'x'", "\"x\"", "%PATH%", "", "a>b", "(x)"] {
            assert_eq!(resolvable_token(c), None, "{c:?}");
        }
    }
}
```

- [ ] **Step 2: Run to see it fail.** Run `cargo test -p claude-dashboard-core command_classifier`. Expected: compile errors.

- [ ] **Step 3: Implement** (above the tests):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Interactive,
    NonInteractive,
}

/// Editors, pagers, monitors, multiplexers, REPLs — TUIs that need a terminal.
const INTERACTIVE_TOOLS: &[&str] = &[
    "vim", "vi", "nvim", "nano", "emacs", "top", "htop", "btop", "less", "more", "tmux", "irb",
    "lazygit", "fzf",
];

/// ssh short options that consume the following token as their value.
const SSH_OPTIONS_WITH_ARGUMENT: &[&str] = &[
    "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o", "-p", "-Q",
    "-R", "-S", "-W", "-w",
];

pub fn leading_token(command: &str) -> &str {
    command.split(' ').next().unwrap_or(command)
}

/// The leading token, only if it looks like a bare command name or path.
/// Anything with shell metacharacters is refused so it is never interpolated
/// into (and run by) the resolver's script.
pub fn resolvable_token(command: &str) -> Option<&str> {
    let t = leading_token(command);
    let ok = !t.is_empty()
        && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '\\' | ':' | '-'));
    ok.then_some(t)
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `\bword\b` over ASCII word chars (`word` is ASCII).
fn has_word(haystack: &str, word: &str) -> bool {
    let h = haystack.as_bytes();
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(word) {
        let i = from + pos;
        let end = i + word.len();
        let before_ok = i == 0 || !is_word_byte(h[i - 1]);
        let after_ok = end >= h.len() || !is_word_byte(h[end]);
        if before_ok && after_ok {
            return true;
        }
        from = i + 1;
        while from < haystack.len() && !haystack.is_char_boundary(from) {
            from += 1;
        }
    }
    false
}

/// Removes `"..."` and `'...'` spans (quotes included), one space per span.
pub fn strip_quoted_regions(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut quote: Option<char> = None;
    for ch in s.chars() {
        match quote {
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                out.push(' ');
            }
            None => out.push(ch),
            Some(q) if ch == q => quote = None,
            Some(_) => {}
        }
    }
    out
}

fn claude_print_mode(command: &str, expanded: &str) -> bool {
    let stripped = strip_quoted_regions(command);
    let has_token = stripped
        .split_whitespace()
        .any(|t| t == "-p" || t == "--print" || t.starts_with("--output-format"));
    let lower = expanded.to_lowercase();
    has_token || lower.contains("--print") || lower.contains("--output-format")
}

/// `ssh host` is interactive; `ssh host cmd…` runs a remote command.
pub fn ssh_interactive(command: &str) -> bool {
    let stripped = strip_quoted_regions(command);
    let tokens: Vec<&str> = stripped.split_whitespace().collect();
    let Some(idx) = tokens.iter().position(|t| *t == "ssh" || t.ends_with("/ssh") || t.ends_with("\\ssh") || t.ends_with("ssh.exe")) else {
        return true;
    };
    let mut i = idx + 1;
    let mut saw_target = false;
    while i < tokens.len() {
        let t = tokens[i];
        if t.starts_with('-') {
            i += if SSH_OPTIONS_WITH_ARGUMENT.contains(&t) { 2 } else { 1 };
            continue;
        }
        if !saw_target {
            saw_target = true;
            i += 1;
            continue;
        }
        return false;
    }
    true
}

pub fn classify(command: &str, expanded: &str) -> CommandKind {
    // Name matching never sees argument prose: only the leading token plus the
    // shell's resolution of it.
    let haystack = format!("{}\n{}", leading_token(command), expanded).to_lowercase();
    if has_word(&haystack, "claude") {
        return if claude_print_mode(command, expanded) {
            CommandKind::NonInteractive
        } else {
            CommandKind::Interactive
        };
    }
    if INTERACTIVE_TOOLS.iter().any(|t| has_word(&haystack, t)) {
        return CommandKind::Interactive;
    }
    if has_word(&haystack, "ssh") {
        return if ssh_interactive(command) {
            CommandKind::Interactive
        } else {
            CommandKind::NonInteractive
        };
    }
    CommandKind::NonInteractive
}
```

Note the test where a function body passes `$args` (PowerShell) and contains `--print`: the expansion is checked for `--print`, so `ccp hello` → NonInteractive. That matches macOS.

- [ ] **Step 4: Run the tests.** Run `cargo test -p claude-dashboard-core command_classifier`. Expected: all pass. Clippy clean.

- [ ] **Step 5: Commit** with message `feat(core): command classifier port` and the trailer.

---

### Task 3: `core::claude_code` + tier-2 sort (TDD)

**Files:**
- Create: `apps/linux/core/src/claude_code.rs`
- Modify: `apps/linux/core/src/rows.rs` (BuildInput, DisplayRow, `build_rows` sort, test helper)
- Modify: `apps/linux/core/src/lib.rs` (`pub mod claude_code;`)
- Modify (compile fixes for the new fields only): `apps/windows/app/src/refresh.rs` (lines 159, 220), `apps/windows/app/src/main.rs` (`fake_rows`, ~line 112), `apps/windows/app/src/model.rs` (test `row()`, ~line 172)

**Interfaces:**
- Produces:
  - `pub fn claude_code::active_email_from(text: &str) -> Option<String>` — `oauthAccount.emailAddress`, trimmed; `None` if missing, empty, not a string, or JSON unparseable
  - `pub fn claude_code::claude_json_path() -> PathBuf` — `%USERPROFILE%\.claude.json` on Windows, `$HOME/.claude.json` elsewhere (falls back to `.claude.json` relative if the var is unset)
  - `pub fn claude_code::active_email() -> Option<String>` — reads the file; any I/O error → `None`
  - `BuildInput { …, pub active_claude_code_email: Option<&'a str> }`
  - `DisplayRow { …, pub is_active_claude_code: bool }` (last field)
  - Sort: pinned first. If **no** row is pinned, the active Claude Code row comes next. Burn-rate key comes after both (`DashboardViewModel.sortStates`). The email match is exact (`account.email == active`, as on macOS and Linux).

- [ ] **Step 1: Write the failing tests** for `claude_code.rs`:

```rust
//! Ports ClaudeCodeAccountDetector.swift: the email Claude Code is signed in
//! as, from `~/.claude.json` (`%USERPROFILE%\.claude.json` on Windows).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_oauth_email() {
        let t = r#"{"oauthAccount":{"emailAddress":"  me@x.com "},"other":1}"#;
        assert_eq!(active_email_from(t).as_deref(), Some("me@x.com"));
    }

    #[test]
    fn missing_or_bad_yields_none() {
        for t in ["", "{", "{}", r#"{"oauthAccount":{}}"#, r#"{"oauthAccount":{"emailAddress":""}}"#,
                  r#"{"oauthAccount":{"emailAddress":42}}"#, r#"{"oauthAccount":null}"#] {
            assert_eq!(active_email_from(t), None, "{t:?}");
        }
    }

    #[test]
    fn path_is_dot_claude_json() {
        assert_eq!(claude_json_path().file_name().unwrap(), ".claude.json");
    }
}
```

Append these to the existing `rows.rs` tests. First add `active_claude_code_email: None,` to the `run` helper's `BuildInput`, then add a `run_with_active` helper that is the same but takes `active: Option<&str>`:

```rust
    fn run_with_active(accounts: &[Account], usage: &[(&str, UsageData)], active: Option<&str>) -> Vec<DisplayRow> {
        let u: HashMap<String, UsageData> =
            usage.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        build_rows(BuildInput {
            accounts,
            usage_by_account: &u,
            errors: &HashMap::new(),
            extension_install_account_ids: &HashSet::new(),
            now_unix_s: NOW,
            active_claude_code_email: active,
        })
    }

    fn with_email(mut a: Account, email: &str) -> Account {
        a.email = Some(email.into());
        a
    }

    #[test]
    fn active_claude_code_account_sorts_after_pins_before_burn() {
        // b burns hardest, but c is the active Claude Code account and nothing is pinned.
        let accts = [
            with_email(acct("a", false, AccountStatus::Active), "a@x"),
            with_email(acct("b", false, AccountStatus::Active), "b@x"),
            with_email(acct("c", false, AccountStatus::Active), "c@x"),
        ];
        let usage = [("a", usage(10.0, Some(3600))), ("b", usage(90.0, Some(3600))), ("c", usage(5.0, Some(3600)))];
        let rows = run_with_active(&accts, &usage, Some("c@x"));
        assert_eq!(ids(&rows), vec!["c", "b", "a"]);
        assert!(rows[0].is_active_claude_code);
        assert!(!rows[1].is_active_claude_code);
    }

    #[test]
    fn any_pin_disables_the_claude_code_tier() {
        let accts = [
            with_email(acct("a", true, AccountStatus::Active), "a@x"),
            with_email(acct("b", false, AccountStatus::Active), "b@x"),
            with_email(acct("c", false, AccountStatus::Active), "c@x"),
        ];
        let usage = [("a", usage(1.0, Some(3600))), ("b", usage(90.0, Some(3600))), ("c", usage(5.0, Some(3600)))];
        let rows = run_with_active(&accts, &usage, Some("c@x"));
        assert_eq!(ids(&rows), vec!["a", "b", "c"]);
        assert!(rows[2].is_active_claude_code, "badge still set");
    }

    #[test]
    fn no_active_email_means_no_badge() {
        let accts = [with_email(acct("a", false, AccountStatus::Active), "a@x")];
        let rows = run_with_active(&accts, &[("a", usage(1.0, Some(3600)))], None);
        assert!(!rows[0].is_active_claude_code);
    }
```

The existing helpers in `rows.rs` are named `acct(id, pinned, status)` and `usage(util, resets_in)`. Before running, check their exact signatures in the test module (around lines 122–140) and adapt the calls if they differ.

- [ ] **Step 2: Run to see it fail.** Run `cargo test -p claude-dashboard-core rows claude_code`. Expected: compile errors.

- [ ] **Step 3: Implement `claude_code.rs`:**

```rust
use std::path::PathBuf;

pub fn active_email_from(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let email = v.get("oauthAccount")?.get("emailAddress")?.as_str()?.trim();
    (!email.is_empty()).then(|| email.to_string())
}

pub fn claude_json_path() -> PathBuf {
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    match std::env::var(home_var) {
        Ok(h) if !h.is_empty() => PathBuf::from(h).join(".claude.json"),
        _ => PathBuf::from(".claude.json"),
    }
}

pub fn active_email() -> Option<String> {
    std::fs::read_to_string(claude_json_path()).ok().and_then(|t| active_email_from(&t))
}
```

- [ ] **Step 4: Implement the `rows.rs` change.** Add the field to `BuildInput` (`pub active_claude_code_email: Option<&'a str>,`) and to `DisplayRow` (`pub is_active_claude_code: bool,` as the last field). In `build_rows`, set `is_active_claude_code: input.active_claude_code_email.is_some_and(|e| a.email.as_deref() == Some(e))`. Carry it in the keyed tuple and replace the sort:

```rust
    let any_pinned = keyed.iter().any(|k| k.0);
    // 1 pinned, 2 active Claude Code (only when nothing is pinned), 3 burn key. Stable.
    keyed.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| if any_pinned { std::cmp::Ordering::Equal } else { b.2.is_active_claude_code.cmp(&a.2.is_active_claude_code) })
            .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
    });
```

Delete the stale `// Tier 2 (active Claude Code) is absent here.` comment.

- [ ] **Step 5: Fix the app-crate constructors.** Add `active_claude_code_email: None,` to both `BuildInput` literals in `refresh.rs`. (The real value is wired in Task 11.) Add `is_active_claude_code: false,` to the `DisplayRow` literals in `main.rs::fake_rows` and `model.rs` tests. Then run `cd apps/windows && cargo build -p claude-dashboard`; expected: builds.

- [ ] **Step 6: Run the tests.** Run `cd apps/linux && cargo test -p claude-dashboard-core` (all) and `cd apps/windows && cargo test --workspace`. Expected: green. Run clippy in both.

- [ ] **Step 7: Commit** with message `feat(core): active Claude Code account detector and sort tier` and the trailer.

---

### Task 4: `core::auto_run` — reset rule and latch (TDD)

**Files:**
- Create: `apps/linux/core/src/auto_run.rs`
- Modify: `apps/linux/core/src/lib.rs` (`pub mod auto_run;`)

**Interfaces:**
- Consumes: `crate::rows::{DisplayRow, WindowView}`
- Produces:
  - `pub fn should_run_saved_command(row: &DisplayRow) -> bool` — true iff the row has usage (`five_hour` or `seven_day` is `Some`) and the 5h **or** 7d window's `resets_at_unix` is `None`.
  - `#[derive(Debug, Default)] pub struct AutoRunLatch` with `new() -> Self`, `due(&mut self, rows: &[DisplayRow], has_command: impl Fn(&str) -> bool) -> Vec<String>`, `armed_count(&self) -> usize`

`due` semantics (Ruling 5), applied per row:
- If `should_run_saved_command(row)` is true, the id is not yet armed, and `has_command(id)` is true: arm it and return it.
- If the rule is true and the id is already armed: return nothing.
- If the rule is false: disarm the id (re-arm).

After the pass, disarm any armed id that is not among `rows`, so a vanished account doesn't hold the latch.

- [ ] **Step 1: Write the failing tests:**

```rust
//! Ports DashboardViewModel's reset monitor (`shouldRunSavedCommand` +
//! `pingedAccounts`, DashboardViewModel.swift:208-211, 278-293). The signal is
//! "a window has no reset time" — the API drops `resets_at` between one cycle
//! ending and the next starting. The latch fires once per such episode.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountPlan, AccountStatus};

    fn w(resets: Option<f64>) -> Option<WindowView> {
        Some(WindowView { utilization: 10.0, resets_at_unix: resets, is_limited: false })
    }

    fn row(id: &str, five: Option<WindowView>, seven: Option<WindowView>) -> DisplayRow {
        DisplayRow {
            account_id: id.into(),
            name: id.into(),
            email: None,
            plan: AccountPlan::Pro,
            status: AccountStatus::Active,
            five_hour: five,
            seven_day: seven,
            fable: None,
            peak_utilization: 10.0,
            burn_projected_seconds: None,
            is_extension_sourced: false,
            error: None,
            last_synced_unix: None,
            is_active_claude_code: false,
        }
    }

    fn reset(id: &str) -> DisplayRow { row(id, w(None), w(Some(5.0))) }
    fn ticking(id: &str) -> DisplayRow { row(id, w(Some(1.0)), w(Some(5.0))) }
    fn all(_: &str) -> bool { true }

    #[test]
    fn rule_needs_usage_and_a_missing_reset() {
        assert!(should_run_saved_command(&reset("a")));
        assert!(should_run_saved_command(&row("a", w(Some(1.0)), w(None))));
        assert!(!should_run_saved_command(&ticking("a")));
        assert!(!should_run_saved_command(&row("a", None, None)), "no usage -> never");
    }

    #[test]
    fn fires_once_per_episode() {
        let mut l = AutoRunLatch::new();
        assert_eq!(l.due(&[reset("a")], all), vec!["a".to_string()]);
        assert!(l.due(&[reset("a")], all).is_empty());
        assert!(l.due(&[reset("a")], all).is_empty());
    }

    #[test]
    fn rearms_only_when_both_windows_tick_again() {
        let mut l = AutoRunLatch::new();
        l.due(&[reset("a")], all);
        assert!(l.due(&[ticking("a")], all).is_empty());
        assert_eq!(l.due(&[reset("a")], all), vec!["a".to_string()]);
    }

    #[test]
    fn never_fires_without_a_saved_command() {
        let mut l = AutoRunLatch::new();
        assert!(l.due(&[reset("a")], |_| false).is_empty());
        assert_eq!(l.armed_count(), 0, "does not arm");
    }

    #[test]
    fn command_saved_mid_episode_fires_next_pass() {
        let mut l = AutoRunLatch::new();
        assert!(l.due(&[reset("a")], |_| false).is_empty());
        assert_eq!(l.due(&[reset("a")], all), vec!["a".to_string()]);
    }

    #[test]
    fn accounts_are_independent() {
        let mut l = AutoRunLatch::new();
        let due = l.due(&[reset("a"), ticking("b"), reset("c")], |id| id != "c");
        assert_eq!(due, vec!["a".to_string()]);
    }

    #[test]
    fn vanished_account_releases_the_latch() {
        let mut l = AutoRunLatch::new();
        l.due(&[reset("a")], all);
        assert_eq!(l.armed_count(), 1);
        l.due(&[ticking("b")], all);
        assert_eq!(l.armed_count(), 0);
    }
}
```

`is_active_claude_code` is the field Task 3 added to `DisplayRow`.

- [ ] **Step 2: Run to see it fail.** Run `cargo test -p claude-dashboard-core auto_run`. Expected: compile errors.

- [ ] **Step 3: Implement:**

```rust
use std::collections::HashSet;

use crate::rows::{DisplayRow, WindowView};

/// `shouldRunSavedCommand(for:)`: usage present and the 5h or 7d window has no reset time.
pub fn should_run_saved_command(row: &DisplayRow) -> bool {
    let missing = |w: &Option<WindowView>| matches!(w, Some(v) if v.resets_at_unix.is_none());
    missing(&row.five_hour) || missing(&row.seven_day)
}

#[derive(Debug, Default)]
pub struct AutoRunLatch {
    pinged: HashSet<String>,
}

impl AutoRunLatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ids whose saved command should run on this pass.
    pub fn due(&mut self, rows: &[DisplayRow], has_command: impl Fn(&str) -> bool) -> Vec<String> {
        let mut out = Vec::new();
        for row in rows {
            let id = row.account_id.as_str();
            if should_run_saved_command(row) {
                if !self.pinged.contains(id) && has_command(id) {
                    self.pinged.insert(id.to_string());
                    out.push(id.to_string());
                }
            } else {
                self.pinged.remove(id);
            }
        }
        let live: HashSet<&str> = rows.iter().map(|r| r.account_id.as_str()).collect();
        self.pinged.retain(|id| live.contains(id.as_str()));
        out
    }

    pub fn armed_count(&self) -> usize {
        self.pinged.len()
    }
}
```

- [ ] **Step 4: Run the tests.** Expected: all pass; clippy clean.
- [ ] **Step 5: Commit** with message `feat(core): auto-run reset latch` and the trailer.

---

### Task 5: `core::run_commands` + `Settings.shell` (TDD)

**Files:**
- Create: `apps/linux/core/src/run_commands.rs`
- Modify: `apps/linux/core/src/settings.rs` (add `shell`; expose the temp+rename writer as `pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String>` and make `save_to` call it)
- Modify: `apps/linux/core/src/lib.rs` (`pub mod run_commands;`)

**Interfaces:**
- Produces:
  - `Settings { …, #[serde(rename = "shell", skip_serializing_if = "Option::is_none", default)] pub shell: Option<String> }`
  - `#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)] pub struct RunCommand { pub command: String, #[serde(rename = "openInTerminal", default)] pub open_in_terminal: bool }`
  - `pub fn run_commands_path() -> PathBuf` → beside `accounts.json`: `run-commands.json`
  - `pub fn load() -> HashMap<String, RunCommand>`; `pub fn get(account_id: &str) -> Option<RunCommand>`; `pub fn set(account_id: &str, rc: &RunCommand) -> Result<(), String>` (a blank `command`, after trim, removes the entry); `pub fn remove(account_id: &str) -> Result<(), String>`
  - Path-explicit twins for tests: `load_at(&Path)`, `set_at(&Path, &str, &RunCommand)`, `remove_at(&Path, &str)`

File shape: `{"version":1,"commands":{"<accountId>":{"command":"…","openInTerminal":false}}}`. A missing or corrupt file, or a wrong version, loads as empty, so `load` never errors. Writes go through `settings::write_atomic`. A process-wide `static RMW: Mutex<()>` is held across each load-modify-save. Only the app writes this file, but several app threads can (a manual save and an account delete).

- [ ] **Step 1: Write the failing tests.** Add to `settings.rs` tests: in `roundtrip`, set `shell: Some("pwsh".into())` in the literal. Add a test `missing_shell_key_loads_none`: write `{"autoRefreshSeconds":60}`, then `load_from(..).shell == None`. In `run_commands.rs`:

```rust
//! Per-account saved run command (macOS `RunCommandSettings`): one command and
//! its "open in terminal" choice per `Account.id`. Deleting the account drops
//! it; a re-sync keeps the id, so it survives that.

#[cfg(test)]
mod tests {
    use super::*;

    fn rc(cmd: &str, term: bool) -> RunCommand {
        RunCommand { command: cmd.into(), open_in_terminal: term }
    }

    #[test]
    fn missing_or_corrupt_loads_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("run-commands.json");
        assert!(load_at(&p).is_empty());
        std::fs::write(&p, "{ nope").unwrap();
        assert!(load_at(&p).is_empty());
        std::fs::write(&p, r#"{"version":2,"commands":{"a":{"command":"x"}}}"#).unwrap();
        assert!(load_at(&p).is_empty(), "unknown version ignored");
    }

    #[test]
    fn set_get_roundtrip_and_independent_keys() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nested").join("run-commands.json");
        set_at(&p, "a", &rc("ccbf", true)).unwrap();
        set_at(&p, "b", &rc("echo hi", false)).unwrap();
        let m = load_at(&p);
        assert_eq!(m.get("a"), Some(&rc("ccbf", true)));
        assert_eq!(m.get("b"), Some(&rc("echo hi", false)));
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1, "no temp litter");
    }

    #[test]
    fn blank_command_removes_entry() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("run-commands.json");
        set_at(&p, "a", &rc("ccbf", false)).unwrap();
        set_at(&p, "a", &rc("   ", false)).unwrap();
        assert!(load_at(&p).get("a").is_none());
    }

    #[test]
    fn remove_drops_only_that_account() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("run-commands.json");
        set_at(&p, "a", &rc("x", false)).unwrap();
        set_at(&p, "b", &rc("y", false)).unwrap();
        remove_at(&p, "a").unwrap();
        remove_at(&p, "missing").unwrap();
        let m = load_at(&p);
        assert!(m.get("a").is_none());
        assert!(m.get("b").is_some());
    }

    #[test]
    fn open_in_terminal_defaults_false() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("run-commands.json");
        std::fs::write(&p, r#"{"version":1,"commands":{"a":{"command":"x"}}}"#).unwrap();
        assert_eq!(load_at(&p).get("a"), Some(&rc("x", false)));
    }

    #[test]
    fn path_is_beside_accounts() {
        assert_eq!(run_commands_path().file_name().unwrap(), "run-commands.json");
        assert_eq!(run_commands_path().parent(), crate::store::accounts_path().parent());
    }
}
```

- [ ] **Step 2: Run to see it fail.** Run `cargo test -p claude-dashboard-core run_commands settings`. Expected: compile errors.

- [ ] **Step 3: Implement.** In `settings.rs`, add the field and `shell: None` in `Default`. Factor the body of `save_to` after `to_string_pretty` into `pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String>` (create parent dirs, temp file `create_new`, `sync_all`, rename, remove temp on error), and have `save_to` call it. `run_commands.rs`:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

const FORMAT_VERSION: u32 = 1;
static RMW: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunCommand {
    pub command: String,
    #[serde(rename = "openInTerminal", default)]
    pub open_in_terminal: bool,
}

#[derive(Serialize, Deserialize)]
struct FileShape {
    version: u32,
    #[serde(default)]
    commands: HashMap<String, RunCommand>,
}

pub fn run_commands_path() -> PathBuf {
    crate::store::accounts_path().with_file_name("run-commands.json")
}

pub fn load_at(path: &Path) -> HashMap<String, RunCommand> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<FileShape>(&t).ok())
        .filter(|f| f.version == FORMAT_VERSION)
        .map(|f| f.commands)
        .unwrap_or_default()
}

fn save_at(path: &Path, commands: HashMap<String, RunCommand>) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&FileShape { version: FORMAT_VERSION, commands })
        .map_err(|e| e.to_string())?;
    crate::settings::write_atomic(path, json.as_bytes())
}

fn modify_at(path: &Path, f: impl FnOnce(&mut HashMap<String, RunCommand>)) -> Result<(), String> {
    let _g = RMW.lock().unwrap_or_else(|p| p.into_inner());
    let mut m = load_at(path);
    f(&mut m);
    save_at(path, m)
}

pub fn set_at(path: &Path, account_id: &str, rc: &RunCommand) -> Result<(), String> {
    modify_at(path, |m| {
        if rc.command.trim().is_empty() {
            m.remove(account_id);
        } else {
            m.insert(account_id.to_string(), rc.clone());
        }
    })
}

pub fn remove_at(path: &Path, account_id: &str) -> Result<(), String> {
    modify_at(path, |m| {
        m.remove(account_id);
    })
}

pub fn load() -> HashMap<String, RunCommand> {
    load_at(&run_commands_path())
}
pub fn get(account_id: &str) -> Option<RunCommand> {
    load().remove(account_id)
}
pub fn set(account_id: &str, rc: &RunCommand) -> Result<(), String> {
    set_at(&run_commands_path(), account_id, rc)
}
pub fn remove(account_id: &str) -> Result<(), String> {
    remove_at(&run_commands_path(), account_id)
}
```

If the existing `Settings` struct literals elsewhere in the repo stop compiling, add `shell: None` to them. Check with `grep -rn "Settings {" apps --include=*.rs`.

- [ ] **Step 4: Run the tests.** Run `cd apps/linux && cargo test -p claude-dashboard-core` and `cd apps/windows && cargo test --workspace`. Expected: green. Clippy both.
- [ ] **Step 5: Commit** with message `feat(core): saved run commands store and shell setting` and the trailer.

---

### Task 6: app `shell.rs` + `terminal.rs` — discovery and argv (TDD)

**Files:**
- Create: `apps/windows/app/src/shell.rs`, `apps/windows/app/src/terminal.rs`
- Modify: `apps/windows/app/src/main.rs` (add `mod shell; mod terminal;`)

**Interfaces:**
- Produces (`shell.rs`):
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ShellKind { Pwsh, WindowsPowerShell, Cmd, GitBash }` with `pub const ALL: [ShellKind; 4]`, `setting_key(self) -> &'static str` (`"pwsh" | "powershell" | "cmd" | "bash"`), `from_setting(&str) -> Option<Self>`, `label(self) -> &'static str` (`"PowerShell 7" | "Windows PowerShell" | "Command Prompt" | "Git Bash"`)
  - `#[derive(Debug, Clone, PartialEq)] pub struct ShellSpec { pub kind: ShellKind, pub exe: PathBuf }`
  - `#[derive(Debug, Clone, PartialEq)] pub struct Invocation { pub program: PathBuf, pub args: Vec<String>, pub raw_args: Option<String> }`. `raw_args` is appended verbatim after `args` via `CommandExt::raw_arg`. `cmd.exe` needs it because it parses its own command line.
  - `pub struct Probe<'a> { pub env: &'a dyn Fn(&str) -> Option<String>, pub exists: &'a dyn Fn(&Path) -> bool }`
  - `pub fn locate(kind: ShellKind, p: &Probe) -> Option<PathBuf>`; `pub fn available(p: &Probe) -> Vec<ShellSpec>` (in `ALL` order); `pub fn resolve(setting: Option<&str>, p: &Probe) -> Option<ShellSpec>`. `resolve` returns the chosen shell if it is available. Otherwise it falls back to the default order Pwsh → WindowsPowerShell → Cmd.
  - `pub fn detect(setting: Option<&str>) -> Option<ShellSpec>` (real env + `Path::exists`); `pub fn detect_available() -> Vec<ShellSpec>`
  - `pub fn run_invocation(spec: &ShellSpec, command: &str) -> Invocation` — a hidden run that loads the profile
  - `pub fn resolve_invocation(spec: &ShellSpec, token: &str) -> Invocation` — the expansion script for an **already validated** token
- Produces (`terminal.rs`):
  - `pub fn wt_path(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf>` → `%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe`
  - `pub fn wt_escape(arg: &str) -> String` — `;` → `\;` (wt treats a bare `;` as its own subcommand separator)
  - `pub fn terminal_invocation(spec: &ShellSpec, command: &str, wt: Option<&Path>, system_root: &Path) -> Invocation`
  - `pub fn launch(inv: &Invocation) -> Result<(), String>` — spawn detached (no job, no wait)

`locate` rules:

| Kind | Where it looks |
|---|---|
| `Pwsh` | Each `PATH` dir (split on `;`, skip empty) containing `pwsh.exe`; else `%ProgramFiles%\PowerShell\7\pwsh.exe` |
| `WindowsPowerShell` | `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe` |
| `Cmd` | `%ComSpec%` if it exists; else `%SystemRoot%\System32\cmd.exe` |
| `GitBash` | `%ProgramFiles%\Git\bin\bash.exe`; else `%LOCALAPPDATA%\Programs\Git\bin\bash.exe`. **Never** `System32\bash.exe`, which is WSL. |

A candidate counts only if `exists` returns true.

`run_invocation` (the profile is loaded in all variants; that is the `source ~/.zshrc` analogue):

| Kind | `args` | `raw_args` |
|---|---|---|
| Pwsh / WindowsPowerShell | `["-NoLogo", "-NonInteractive", "-Command", "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; <command>"]` | `None` |
| Cmd | `["/s", "/c"]` | `Some("\"<command>\"")` (no `/d`, so the AutoRun "profile" applies) |
| GitBash | `["-c", "source ~/.bashrc 2>/dev/null\n<command>"]` | `None` |

`resolve_invocation` (one level of expansion, like `whence`):

| Kind | Invocation |
|---|---|
| Pwsh / WindowsPowerShell | `["-NoLogo", "-NonInteractive", "-Command", "$c = Get-Command -Name '<token>' -ErrorAction SilentlyContinue \| Select-Object -First 1; if ($c) { $c.Definition; $c.Source }"]` |
| Cmd | `args ["/s", "/c"]`, `raw_args Some("\"where <token>\"")` |
| GitBash | `["-ic", "type <token> 2>/dev/null"]` |

`terminal_invocation`. The interactive tail per kind:

| Kind | Tail |
|---|---|
| Pwsh / WinPS | `[exe, "-NoLogo", "-NoExit", "-Command", command]` |
| Cmd | `[exe, "/k", command]` |
| GitBash | `[exe, "-l", "-i", "-c", "<command>; exec bash -l -i"]` |

With `wt = Some(p)`: `program = p`, `args = ["new-tab", "--title", "Claude Dashboard", "--"]` followed by the tail with **every tail element passed through `wt_escape`**. With `wt = None`: `program = system_root\System32\conhost.exe`, `args = tail`. `raw_args` is `None` in both cases.

- [ ] **Step 1: Write the failing tests** (in `shell.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    struct Fake { env: HashMap<&'static str, String>, files: HashSet<PathBuf> }
    impl Fake {
        fn new(env: &[(&'static str, &str)], files: &[&str]) -> Self {
            Fake {
                env: env.iter().map(|(k, v)| (*k, v.to_string())).collect(),
                files: files.iter().map(PathBuf::from).collect(),
            }
        }
        fn with<R>(&self, f: impl FnOnce(&Probe) -> R) -> R {
            let env = |k: &str| self.env.get(k).cloned();
            let exists = |p: &Path| self.files.contains(p);
            f(&Probe { env: &env, exists: &exists })
        }
    }

    const BASE: &[(&str, &str)] = &[
        ("SystemRoot", "C:\\Windows"),
        ("ProgramFiles", "C:\\Program Files"),
        ("LOCALAPPDATA", "C:\\Users\\u\\AppData\\Local"),
        ("ComSpec", "C:\\Windows\\system32\\cmd.exe"),
        ("PATH", "C:\\Windows\\system32;;C:\\tools"),
    ];
    const WINPS: &str = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
    const CMD: &str = "C:\\Windows\\system32\\cmd.exe";

    #[test]
    fn pwsh_found_on_path_first() {
        let f = Fake::new(BASE, &["C:\\tools\\pwsh.exe", "C:\\Program Files\\PowerShell\\7\\pwsh.exe"]);
        assert_eq!(f.with(|p| locate(ShellKind::Pwsh, p)), Some(PathBuf::from("C:\\tools\\pwsh.exe")));
    }

    #[test]
    fn pwsh_falls_back_to_program_files() {
        let f = Fake::new(BASE, &["C:\\Program Files\\PowerShell\\7\\pwsh.exe"]);
        assert_eq!(f.with(|p| locate(ShellKind::Pwsh, p)),
                   Some(PathBuf::from("C:\\Program Files\\PowerShell\\7\\pwsh.exe")));
    }

    #[test]
    fn git_bash_never_picks_wsl_bash() {
        let f = Fake::new(BASE, &["C:\\Windows\\System32\\bash.exe"]);
        assert_eq!(f.with(|p| locate(ShellKind::GitBash, p)), None);
        let f = Fake::new(BASE, &["C:\\Program Files\\Git\\bin\\bash.exe"]);
        assert_eq!(f.with(|p| locate(ShellKind::GitBash, p)),
                   Some(PathBuf::from("C:\\Program Files\\Git\\bin\\bash.exe")));
    }

    #[test]
    fn default_prefers_pwsh_then_windows_powershell() {
        let f = Fake::new(BASE, &[WINPS, CMD]);
        assert_eq!(f.with(|p| resolve(None, p)).map(|s| s.kind), Some(ShellKind::WindowsPowerShell));
        let f = Fake::new(BASE, &["C:\\tools\\pwsh.exe", WINPS, CMD]);
        assert_eq!(f.with(|p| resolve(None, p)).map(|s| s.kind), Some(ShellKind::Pwsh));
    }

    #[test]
    fn chosen_shell_used_when_available_else_default() {
        let f = Fake::new(BASE, &[WINPS, CMD]);
        assert_eq!(f.with(|p| resolve(Some("cmd"), p)).map(|s| s.kind), Some(ShellKind::Cmd));
        assert_eq!(f.with(|p| resolve(Some("bash"), p)).map(|s| s.kind), Some(ShellKind::WindowsPowerShell));
        assert_eq!(f.with(|p| resolve(Some("garbage"), p)).map(|s| s.kind), Some(ShellKind::WindowsPowerShell));
    }

    #[test]
    fn nothing_found_is_none() {
        let f = Fake::new(&[], &[]);
        assert_eq!(f.with(|p| resolve(None, p)), None);
        assert!(f.with(available).is_empty());
    }

    #[test]
    fn setting_keys_roundtrip() {
        for k in ShellKind::ALL {
            assert_eq!(ShellKind::from_setting(k.setting_key()), Some(k));
        }
        assert_eq!(ShellKind::from_setting("zsh"), None);
    }

    fn spec(kind: ShellKind, exe: &str) -> ShellSpec { ShellSpec { kind, exe: PathBuf::from(exe) } }

    #[test]
    fn run_invocations_load_profile_and_pass_command() {
        let i = run_invocation(&spec(ShellKind::Pwsh, "pwsh.exe"), "ccbf --x");
        assert_eq!(i.args[..3], ["-NoLogo", "-NonInteractive", "-Command"]);
        assert!(i.args[3].ends_with("; ccbf --x"));
        assert!(!i.args.iter().any(|a| a == "-NoProfile"));
        let i = run_invocation(&spec(ShellKind::Cmd, CMD), "echo a & echo b");
        assert_eq!(i.args, ["/s", "/c"]);
        assert_eq!(i.raw_args.as_deref(), Some("\"echo a & echo b\""));
        let i = run_invocation(&spec(ShellKind::GitBash, "bash.exe"), "ls");
        assert_eq!(i.args, ["-c", "source ~/.bashrc 2>/dev/null\nls"]);
    }

    #[test]
    fn resolve_invocations_embed_only_the_token() {
        let i = resolve_invocation(&spec(ShellKind::Pwsh, "pwsh.exe"), "ccbf");
        assert!(i.args[3].contains("Get-Command -Name 'ccbf'"));
        let i = resolve_invocation(&spec(ShellKind::Cmd, CMD), "git");
        assert_eq!(i.raw_args.as_deref(), Some("\"where git\""));
        let i = resolve_invocation(&spec(ShellKind::GitBash, "bash.exe"), "ll");
        assert_eq!(i.args, ["-ic", "type ll 2>/dev/null"]);
    }
}
```

And in `terminal.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{ShellKind, ShellSpec};

    fn spec(kind: ShellKind, exe: &str) -> ShellSpec { ShellSpec { kind, exe: PathBuf::from(exe) } }
    const ROOT: &str = "C:\\Windows";

    #[test]
    fn wt_escapes_semicolons() {
        assert_eq!(wt_escape("a; b;c"), "a\\; b\\;c");
        assert_eq!(wt_escape("plain"), "plain");
    }

    #[test]
    fn wt_new_tab_with_noexit_pwsh() {
        let wt = PathBuf::from("C:\\wt.exe");
        let i = terminal_invocation(&spec(ShellKind::Pwsh, "C:\\pwsh.exe"), "claude; echo done", Some(&wt), Path::new(ROOT));
        assert_eq!(i.program, wt);
        assert_eq!(i.args, ["new-tab", "--title", "Claude Dashboard", "--", "C:\\pwsh.exe",
                            "-NoLogo", "-NoExit", "-Command", "claude\\; echo done"]);
        assert_eq!(i.raw_args, None);
    }

    #[test]
    fn conhost_fallback_without_wt() {
        let i = terminal_invocation(&spec(ShellKind::Cmd, "C:\\cmd.exe"), "dir", None, Path::new(ROOT));
        assert_eq!(i.program, PathBuf::from("C:\\Windows\\System32\\conhost.exe"));
        assert_eq!(i.args, ["C:\\cmd.exe", "/k", "dir"]);
    }

    #[test]
    fn git_bash_keeps_an_interactive_shell_after_the_command() {
        let i = terminal_invocation(&spec(ShellKind::GitBash, "C:\\bash.exe"), "htop", None, Path::new(ROOT));
        assert_eq!(i.args, ["C:\\bash.exe", "-l", "-i", "-c", "htop; exec bash -l -i"]);
    }

    #[test]
    fn wt_path_under_windowsapps() {
        let env = |k: &str| (k == "LOCALAPPDATA").then(|| "C:\\L".to_string());
        assert_eq!(wt_path(&env), Some(PathBuf::from("C:\\L\\Microsoft\\WindowsApps\\wt.exe")));
    }
}
```

`wt_path` returns the candidate path without checking that it exists. The caller, `commands::launch_in_terminal` (Task 8), passes `wt_path(..).filter(|p| p.exists())`.

- [ ] **Step 2: Run to see it fail.** Run `cd apps/windows && cargo test -p claude-dashboard shell terminal`. Expected: compile errors.

- [ ] **Step 3: Implement** both modules from the tables above. Give `detect` / `detect_available` a real probe: `env = |k| std::env::var(k).ok().filter(|v| !v.is_empty())`, `exists = |p| p.is_file()`. `launch`:

```rust
pub fn launch(inv: &Invocation) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(&inv.program);
    c.args(&inv.args);
    if let Some(raw) = &inv.raw_args {
        c.raw_arg(raw);
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        c.current_dir(home);
    }
    c.spawn().map(|_| ()).map_err(|e| format!("terminal launch failed: {e}"))
}
```

Until Task 8 uses them, mark the new public items with a temporary `#![allow(dead_code)]` at the top of each module. **Task 8 removes it** (gotcha #6).

- [ ] **Step 4: Run the tests.** Run `cargo test -p claude-dashboard` and `cargo clippy --all-targets -- -D warnings`. Expected: green.
- [ ] **Step 5: Commit** with message `feat(windows): shell discovery and terminal launch argv` and the trailer.

---

### Task 7: app `runner.rs` — Job Object runner (TDD against `cmd.exe`)

**Files:**
- Create: `apps/windows/app/src/runner.rs`
- Modify: `apps/windows/app/Cargo.toml` (append `"Win32_System_JobObjects", "Win32_System_Diagnostics_ToolHelp"` to the `windows-sys` features list)
- Modify: `apps/windows/app/src/main.rs` (`mod runner;`)

**Interfaces:**
- Consumes: `crate::shell::Invocation`; `claude_dashboard_core::command_log::{CommandStatus, bounded_tail, MAX_OUTPUT_BYTES}`
- Produces:
  - `#[derive(Clone, Default)] pub struct CancelToken(Arc<AtomicBool>)` with `new()`, `cancel(&self)`, `is_cancelled(&self) -> bool`
  - `#[derive(Debug, Clone, PartialEq)] pub struct RunOutcome { pub status: CommandStatus, pub exit_code: Option<i32>, pub output_tail: String, pub started_unix: i64, pub finished_unix: i64 }`
  - `pub fn run(inv: &Invocation, timeout: Duration, cancel: &CancelToken, on_output: &(dyn Fn(&str) + Sync)) -> RunOutcome`. It blocks the calling thread until the child (and its whole tree) is gone, so callers run it on a worker.

Algorithm:
1. Create a job: `CreateJobObjectW(null, null)`, then `SetInformationJobObject(job, JobObjectExtendedLimitInformation, &JOBOBJECT_EXTENDED_LIMIT_INFORMATION { BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, .. zeroed })`. Wrap the handle in `struct Job(HANDLE)` with `terminate(&self)` (`TerminateJobObject(h, 1)`), `close(&mut self)` (`CloseHandle` once, then null), and a `Drop` that closes.
2. Build a `std::process::Command` from the invocation (`args`, then `raw_arg(raw_args)`). Set `current_dir(%USERPROFILE%)` if set, `stdin(Stdio::null())`, piped stdout/stderr, and `creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)` (`0x0000_0004 | 0x0800_0000`).
3. On a `spawn` error, return `LaunchFailed`, `exit_code None`, and `output_tail = format!("launch failed: {e}")`.
4. Call `AssignProcessToJobObject(job, child.as_raw_handle())`. Then resume: take a `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)`, walk `Thread32First/Next`, and for each entry with `th32OwnerProcessID == child.id()`, `OpenThread(THREAD_SUSPEND_RESUME, 0, tid)` → `ResumeThread` → `CloseHandle`. If the assign fails or no thread was resumed, `child.kill()` and return `LaunchFailed` with the reason.
5. Inside `std::thread::scope`, spawn two readers (stdout, stderr). Each reads 4096-byte chunks, decodes with `String::from_utf8_lossy`, calls `on_output(&chunk)`, and appends to a shared `Mutex<String>` that is re-trimmed with `bounded_tail(.., MAX_OUTPUT_BYTES)` after each append. A reader stops on `Ok(0)` or an error.
6. Main loop (still inside the scope), polling every 50 ms with `child.try_wait()`. On `Some(st)` → break with `st.code()`. If not yet killed and `cancel.is_cancelled()` → status `Cancelled` and `job.terminate()`. Else if not yet killed and past the deadline → status `TimedOut` and `job.terminate()`. A `killed` flag prevents terminating twice; the loop continues until the child is reaped. `try_wait` `Err` → break.
7. After the loop: `job.close()`. `KILL_ON_JOB_CLOSE` ends any background leftovers (Ruling 3), so the readers reach EOF. The scope then joins them.
8. `exit_code` is `Some(code)` only when the status is `Exited`. `started_unix` and `finished_unix` come from `SystemTime::now()`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Instant;

    fn cmd(script: &str) -> Invocation {
        let exe = std::env::var("ComSpec").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".into());
        Invocation { program: PathBuf::from(exe), args: vec!["/d".into(), "/s".into(), "/c".into()],
                     raw_args: Some(format!("\"{script}\"")) }
    }
    fn quiet(_: &str) {}

    #[test]
    fn captures_output_and_exit_zero() {
        let seen = Mutex::new(String::new());
        let out = run(&cmd("echo hello"), Duration::from_secs(20), &CancelToken::new(),
                      &|s| seen.lock().unwrap().push_str(s));
        assert_eq!(out.status, CommandStatus::Exited);
        assert_eq!(out.exit_code, Some(0));
        assert!(out.output_tail.contains("hello"));
        assert!(seen.lock().unwrap().contains("hello"));
        assert!(out.finished_unix >= out.started_unix);
    }

    #[test]
    fn reports_nonzero_exit() {
        let out = run(&cmd("exit 3"), Duration::from_secs(20), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::Exited);
        assert_eq!(out.exit_code, Some(3));
    }

    #[test]
    fn timeout_kills_tree() {
        let t = Instant::now();
        let out = run(&cmd("ping -n 30 127.0.0.1 > nul"), Duration::from_secs(1), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::TimedOut);
        assert_eq!(out.exit_code, None);
        assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    }

    #[test]
    fn cancel_kills_tree() {
        let token = CancelToken::new();
        let c = token.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(300)); c.cancel(); });
        let t = Instant::now();
        let out = run(&cmd("ping -n 30 127.0.0.1"), Duration::from_secs(60), &token, &quiet);
        assert_eq!(out.status, CommandStatus::Cancelled);
        assert!(t.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn background_grandchild_does_not_hang() {
        // cmd exits at once; the ping inherits the output pipe. Without the job
        // closing, the readers would block ~30s.
        let t = Instant::now();
        let out = run(&cmd("start /b ping -n 30 127.0.0.1"), Duration::from_secs(60), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::Exited);
        assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    }

    #[test]
    fn stdin_is_null_so_reads_return() {
        let t = Instant::now();
        let out = run(&cmd("set /p x=& echo after"), Duration::from_secs(20), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::Exited);
        assert!(out.output_tail.contains("after"));
        assert!(t.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn chatty_output_is_bounded() {
        let out = run(&cmd("for /l %i in (1,1,3000) do @echo line%i"), Duration::from_secs(60),
                      &CancelToken::new(), &quiet);
        assert!(out.output_tail.len() <= MAX_OUTPUT_BYTES);
        assert!(out.output_tail.trim_end().ends_with("line3000"));
    }

    #[test]
    fn missing_program_is_launch_failed() {
        let inv = Invocation { program: PathBuf::from("C:\\definitely\\missing.exe"), args: vec![], raw_args: None };
        let out = run(&inv, Duration::from_secs(5), &CancelToken::new(), &quiet);
        assert_eq!(out.status, CommandStatus::LaunchFailed);
        assert!(out.output_tail.starts_with("launch failed"));
    }
}
```

These tests use `/d` so a developer's cmd AutoRun can't interfere. Only test invocations do that; production `run_invocation` keeps the AutoRun profile.

- [ ] **Step 2: Run to see it fail.** Run `cargo test -p claude-dashboard runner`. Expected: compile errors.
- [ ] **Step 3: Implement** following the algorithm. Import `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`, `JobObjectExtendedLimitInformation`, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, `CreateJobObjectW`, `SetInformationJobObject`, `AssignProcessToJobObject` and `TerminateJobObject` from `windows_sys::Win32::System::JobObjects`. Import `CreateToolhelp32Snapshot`, `Thread32First`, `Thread32Next`, `THREADENTRY32` and `TH32CS_SNAPTHREAD` from `windows_sys::Win32::System::Diagnostics::ToolHelp`. Import `OpenThread`, `ResumeThread` and `THREAD_SUSPEND_RESUME` from `windows_sys::Win32::System::Threading`. Import `CloseHandle`, `HANDLE` and `INVALID_HANDLE_VALUE` from `windows_sys::Win32::Foundation`. Set `THREADENTRY32.dwSize = size_of::<THREADENTRY32>() as u32` before `Thread32First`. Every `unsafe` block gets a one-line `// SAFETY:` comment.
- [ ] **Step 4: Run the tests.** Run `cargo test -p claude-dashboard runner -- --test-threads=4`. Expected: 8 pass. Then the full `cargo test --workspace` and clippy.
- [ ] **Step 5: Commit** with message `feat(windows): Job Object command runner` and the trailer.

---

### Task 8: app `commands.rs` — run + record, terminal handoff, classify (TDD)

**Files:**
- Create: `apps/windows/app/src/commands.rs`
- Modify: `apps/windows/app/src/main.rs` (`mod commands;`)
- Modify: `apps/windows/app/src/shell.rs`, `terminal.rs` (remove the temporary `#![allow(dead_code)]`, plus anything still unused once this task lands; keep only what is used)

**Interfaces:**
- Consumes: Tasks 1, 2, 6, 7.
- Produces:
  - `pub const RUN_TIMEOUT: Duration = Duration::from_secs(60); pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);`
  - `pub fn execute(command: &str, account_id: Option<&str>, trigger: CommandTrigger, shell: Option<&ShellSpec>, cancel: &CancelToken, on_output: &(dyn Fn(&str) + Sync)) -> RunOutcome`. With no shell, it returns `LaunchFailed` and the tail `"launch failed: no shell found"`. Otherwise it calls `runner::run(&shell::run_invocation(spec, command), RUN_TIMEOUT, ..)`. **It always records one row** (`CommandLogStore::open()`; a store error is `eprintln!`ed, never surfaced).
  - `pub fn launch_in_terminal(command: &str, account_id: Option<&str>, trigger: CommandTrigger, shell: Option<&ShellSpec>) -> CommandStatus`. It builds `terminal::terminal_invocation(spec, command, wt_path(real_env).filter(|p| p.is_file()).as_deref(), system_root)` and calls `terminal::launch`. On success it records `LaunchedInTerminal` with no output. On error, or with no shell, it records `LaunchFailed` with the message as output. `finished_unix` is `Some(now)`; `exit_code` is `None`.
  - `pub fn classify(command: &str, shell: Option<&ShellSpec>) -> CommandKind`. If `resolvable_token(command)` is `Some(tok)` and a shell exists, `expanded = runner::run(&shell::resolve_invocation(spec, tok), RESOLVE_TIMEOUT, &CancelToken::new(), &|_| {}).output_tail` (only when the status is `Exited`, else `""`). Otherwise `expanded = ""`. Then it calls `command_classifier::classify(command, &expanded)`. The resolver run is **not** recorded.
  - A private `fn record(e: &NewEntry)`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::command_log::CommandLogStore;
    use std::path::PathBuf;

    fn cmd_shell() -> ShellSpec {
        let exe = std::env::var("ComSpec").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".into());
        ShellSpec { kind: ShellKind::Cmd, exe: PathBuf::from(exe) }
    }

    fn with_temp_data<R>(f: impl FnOnce() -> R) -> R {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        f()
    }

    #[test]
    fn execute_records_one_row() {
        with_temp_data(|| {
            let out = execute("echo hi", Some("acct-1"), CommandTrigger::AutoReset, Some(&cmd_shell()),
                              &CancelToken::new(), &|_| {});
            assert_eq!(out.exit_code, Some(0));
            let rows = CommandLogStore::open().unwrap().recent(10);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].account_id.as_deref(), Some("acct-1"));
            assert_eq!(rows[0].trigger, CommandTrigger::AutoReset);
            assert_eq!(rows[0].status, CommandStatus::Exited);
            assert!(rows[0].output.as_deref().unwrap_or("").contains("hi"));
        });
    }

    #[test]
    fn no_shell_records_launch_failed() {
        with_temp_data(|| {
            let out = execute("echo hi", None, CommandTrigger::Manual, None, &CancelToken::new(), &|_| {});
            assert_eq!(out.status, CommandStatus::LaunchFailed);
            let r = &CommandLogStore::open().unwrap().recent(1)[0];
            assert_eq!(r.status, CommandStatus::LaunchFailed);
            assert_eq!(r.output.as_deref(), Some("launch failed: no shell found"));
        });
    }

    #[test]
    fn terminal_without_shell_records_launch_failed() {
        with_temp_data(|| {
            assert_eq!(launch_in_terminal("htop", Some("a"), CommandTrigger::Manual, None),
                       CommandStatus::LaunchFailed);
            assert_eq!(CommandLogStore::open().unwrap().recent(1)[0].status, CommandStatus::LaunchFailed);
        });
    }

    #[test]
    fn classify_uses_first_token_when_resolution_fails() {
        // `where vim` finds nothing on CI -> expansion empty -> leading token decides.
        assert_eq!(classify("vim notes.md", Some(&cmd_shell())), CommandKind::Interactive);
        assert_eq!(classify("echo vim", Some(&cmd_shell())), CommandKind::NonInteractive);
        // Metacharacters are never resolved, still classified.
        assert_eq!(classify("$(vim)", Some(&cmd_shell())), CommandKind::NonInteractive);
        assert_eq!(classify("htop", None), CommandKind::Interactive);
    }
}
```

The `launch_in_terminal` success path opens a real window, so it is **not** unit-tested; it is covered by the Task 10 run-check.

- [ ] **Step 2: Run to see it fail.** Run `cargo test -p claude-dashboard commands`. Expected: compile errors.
- [ ] **Step 3: Implement.** Use the current Unix time in seconds for `started_unix` in the terminal path; the `execute` path takes both from `RunOutcome`. The no-shell `execute` path records `started_unix == finished_unix == now`.
- [ ] **Step 4: Run.** Run `cargo test --workspace` and `cargo clippy --all-targets -- -D warnings`. Expected: green, no `dead_code` allowances left in `shell.rs`/`terminal.rs`. Leave a `#[allow(dead_code)]` only on items Tasks 10–11 will use (`classify`, `launch_in_terminal`), each with `// used by run_command.rs (Task 10)`. Task 10 removes them.
- [ ] **Step 5: Commit** with message `feat(windows): run, record and classify commands` and the trailer.

---

### Task 9: Command Log pane (GUI + pure mapping) — enables Tools › Command Log

**Files:**
- Create: `apps/windows/app/ui/command_log.slint`, `apps/windows/app/src/log_view.rs`
- Modify: `apps/windows/app/ui/app.slint` (import; AppWindow properties/callbacks; `changed selection`; sidebar row lines 357–360; the "Coming soon" catch-all at line 413; the pane)
- Modify: `apps/windows/app/src/main.rs` (`mod log_view;`, `log_view::install(&app);`)

**Interfaces:**
- Consumes: `core::command_log::{CommandLogStore, CommandLogEntry, CommandStatus, CommandTrigger}`, `core::store::load_accounts`, `model::local_offset_s`.
- Produces:
  - Slint `export struct UiLogEntry { id: int, time: string, trigger: string, trigger-color: color, account: string, command: string, status: string, status-color: color, duration: string, output: string }`
  - Slint `export component CommandLogPane` with `in property <[UiLogEntry]> entries; callback reload(); callback clear();`
  - AppWindow: `in property <[UiLogEntry]> command-log; callback command-log-open(); callback command-log-clear();`
  - Rust: `pub fn format_log_time(local_unix: i64) -> String` (`"2026-10-05 10:42:03"`), `pub fn duration_text(e: &CommandLogEntry) -> String` (`"2.0s"`, `"—"` when unfinished), `pub fn status_text(e) -> String` (`Exited` → `"exit N"` or `"—"`; else the label), `pub fn status_rgb(e) -> (u8, u8, u8)`, `pub fn trigger_rgb(t) -> (u8, u8, u8)`, `pub fn account_label(id: Option<&str>, names: &HashMap<String, String>) -> String` (`None` → `"—"`, unknown → `"Deleted account"`), `pub fn to_ui_entry(e, names, offset_s) -> UiLogEntry`, `pub fn install(app: &AppWindow)`, `pub fn reload(weak: &slint::Weak<AppWindow>)`.

Colors (RGB): trigger Manual `(10,132,255)`, AutoReset `(191,90,242)`, AutoEmpty `(255,159,10)`. Status: exit 0 `(48,209,88)`; nonzero exit `(255,69,58)`; `Exited` with no code, `Cancelled` → `(142,142,147)`; `TimedOut` `(255,159,10)`; `LaunchedInTerminal` `(10,132,255)`; `LaunchFailed` `(255,69,58)`.

- [ ] **Step 1: Write the failing pure tests** in `log_view.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn e(status: CommandStatus, exit: Option<i32>, finished: Option<i64>) -> CommandLogEntry {
        CommandLogEntry { id: 1, account_id: Some("a".into()), command: "x".into(),
            trigger: CommandTrigger::Manual, started_unix: 100, finished_unix: finished,
            exit_code: exit, status, output: None }
    }

    #[test]
    fn time_is_civil_local() {
        assert_eq!(format_log_time(0), "1970-01-01 00:00:00");
        assert_eq!(format_log_time(1_791_196_923), "2026-10-05 10:42:03");
        assert_eq!(format_log_time(951_782_400), "2000-02-29 00:00:00");
    }

    #[test]
    fn duration_and_status_text() {
        assert_eq!(duration_text(&e(CommandStatus::Exited, Some(0), Some(102))), "2.0s");
        assert_eq!(duration_text(&e(CommandStatus::Exited, Some(0), None)), "—");
        assert_eq!(duration_text(&e(CommandStatus::Exited, Some(0), Some(90))), "0.0s", "clock skew clamps");
        assert_eq!(status_text(&e(CommandStatus::Exited, Some(3), Some(1))), "exit 3");
        assert_eq!(status_text(&e(CommandStatus::Exited, None, Some(1))), "—");
        assert_eq!(status_text(&e(CommandStatus::TimedOut, None, Some(1))), "Timed out");
    }

    #[test]
    fn status_colors() {
        assert_eq!(status_rgb(&e(CommandStatus::Exited, Some(0), None)), (48, 209, 88));
        assert_eq!(status_rgb(&e(CommandStatus::Exited, Some(1), None)), (255, 69, 58));
        assert_eq!(status_rgb(&e(CommandStatus::LaunchFailed, None, None)), (255, 69, 58));
        assert_eq!(trigger_rgb(CommandTrigger::AutoReset), (191, 90, 242));
    }

    #[test]
    fn account_labels() {
        let names: HashMap<String, String> = [("a".to_string(), "work".to_string())].into();
        assert_eq!(account_label(Some("a"), &names), "work");
        assert_eq!(account_label(Some("gone"), &names), "Deleted account");
        assert_eq!(account_label(None, &names), "—");
    }
}
```

`1_791_196_923` must be `2026-10-05 10:42:03` UTC. The implementer verifies that constant against `days_from_civil` before relying on it, and if it's wrong fixes the **test constant**, never the algorithm. Use the standard Howard Hinnant `civil_from_days`.

- [ ] **Step 2: Run to see it fail.** Run `cargo test -p claude-dashboard log_view`. Expected: compile errors.
- [ ] **Step 3: Implement the pure functions**, plus `to_ui_entry`, which applies `format_log_time(e.started_unix + offset_s as i64)`. `output` is `e.output.unwrap_or_default()`, or `"No output captured"` if empty. Implement the glue:

```rust
pub fn reload(weak: &slint::Weak<AppWindow>) {
    let weak = weak.clone();
    std::thread::spawn(move || {
        let entries = CommandLogStore::open().map(|s| s.recent(500)).unwrap_or_default();
        let names: HashMap<String, String> = claude_dashboard_core::store::load_accounts()
            .unwrap_or_default().into_iter().map(|a| (a.id, a.name)).collect();
        let offset = crate::model::local_offset_s();
        let rows: Vec<_> = entries.iter().map(|e| to_ui_entry(e, &names, offset)).collect();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(a) = weak.upgrade() {
                a.set_command_log(slint::ModelRc::new(slint::VecModel::from(rows)));
            }
        });
    });
}

pub fn install(app: &AppWindow) {
    let w = app.as_weak();
    app.on_command_log_open(move || reload(&w));
    let w = app.as_weak();
    app.on_command_log_clear(move || {
        let w = w.clone();
        std::thread::spawn(move || {
            if let Ok(s) = CommandLogStore::open() {
                if let Err(e) = s.clear() { eprintln!("clear command log failed: {e}"); }
            }
            reload(&w);
        });
    });
}
```

`UiLogEntry` is generated from Slint, so `to_ui_entry` builds it with `slint::Color::from_rgb_u8`.

- [ ] **Step 4: Write `ui/command_log.slint`.** It contains a header row with **Refresh** and **Clear All** (Clear asks first: clicking shows inline "Clear all command logs?" with **Clear** / **Cancel**), a `Flickable` list, and an empty state. A row shows time · trigger badge · account on the first line, the command in a monospace font (elided) on the second, and status and duration right-aligned. Clicking a row toggles its output in a monospace `Text` with `wrap: word-wrap`. Slint has no tooltip, so this replaces the macOS `.help`.

```slint
import { Theme } from "theme.slint";

export struct UiLogEntry {
    id: int, time: string, trigger: string, trigger-color: color, account: string,
    command: string, status: string, status-color: color, duration: string, output: string,
}

component LogButton inherits Rectangle {
    in property <string> label;
    in property <bool> enabled: true;
    in property <bool> danger: false;
    callback clicked();
    height: 28px;
    width: t.preferred-width + 24px;
    border-radius: 6px;
    border-width: 1px;
    border-color: Theme.card-border;
    background: touch.has-hover && root.enabled ? Theme.accent.with-alpha(0.15) : Theme.card-bg;
    opacity: root.enabled ? 1.0 : 0.45;
    t := Text { text: root.label; color: root.danger ? Theme.danger : Theme.accent;
                horizontal-alignment: center; vertical-alignment: center; }
    touch := TouchArea { enabled: root.enabled; clicked => { root.clicked(); } }
}

component LogRow inherits Rectangle {
    in property <UiLogEntry> e;
    property <bool> expanded: false;
    height: col.preferred-height;
    background: touch.has-hover ? Theme.accent.with-alpha(0.06) : transparent;
    touch := TouchArea { clicked => { root.expanded = !root.expanded; } }
    col := VerticalLayout {
        padding-left: 16px; padding-right: 16px; padding-top: 10px; padding-bottom: 10px;
        spacing: 4px;
        HorizontalLayout {
            spacing: 8px;
            Text { text: root.e.time; color: Theme.text-secondary; font-size: 11px; vertical-alignment: center; }
            Rectangle {
                width: trig.preferred-width + 12px; height: 18px; border-radius: 9px;
                background: root.e.trigger-color.with-alpha(0.18);
                trig := Text { text: root.e.trigger; color: root.e.trigger-color; font-size: 10px;
                               font-weight: 600; horizontal-alignment: center; vertical-alignment: center; }
            }
            Text { text: root.e.account; color: Theme.text-primary; font-size: 11px; font-weight: 500;
                   vertical-alignment: center; horizontal-stretch: 1; overflow: elide; }
            Text { text: root.e.status; color: root.e.status-color; font-size: 11px; font-weight: 600;
                   vertical-alignment: center; }
            Text { text: root.e.duration; color: Theme.text-secondary; font-size: 10px; vertical-alignment: center; }
        }
        Text { text: root.e.command; color: Theme.text-primary; font-family: "Cascadia Mono, Consolas";
               font-size: 13px; overflow: elide; }
        if root.expanded: Text { text: root.e.output; color: Theme.text-secondary;
               font-family: "Cascadia Mono, Consolas"; font-size: 11px; wrap: word-wrap; }
        Rectangle { height: 1px; background: Theme.card-border; }
    }
}

export component CommandLogPane inherits VerticalLayout {
    in property <[UiLogEntry]> entries;
    callback reload();
    callback clear();
    property <bool> confirming: false;
    spacing: 10px;
    HorizontalLayout {
        spacing: 8px;
        alignment: end;
        if !root.confirming: LogButton { label: "Refresh"; clicked => { root.reload(); } }
        if !root.confirming: LogButton { label: "Clear All"; danger: true; enabled: root.entries.length > 0;
                                         clicked => { root.confirming = true; } }
        if root.confirming: Text { text: "Clear all command logs?"; color: Theme.text-primary; vertical-alignment: center; }
        if root.confirming: LogButton { label: "Clear"; danger: true;
                                        clicked => { root.confirming = false; root.clear(); } }
        if root.confirming: LogButton { label: "Cancel"; clicked => { root.confirming = false; } }
    }
    if root.entries.length == 0: Rectangle {
        vertical-stretch: 1;
        Text { text: "No commands have run yet."; color: Theme.text-secondary;
               horizontal-alignment: center; vertical-alignment: center; }
    }
    if root.entries.length > 0: Rectangle {
        vertical-stretch: 1;
        background: Theme.card-bg; border-radius: 8px; border-width: 1px; border-color: Theme.card-border;
        clip: true;
        Flickable {
            content-height: list.preferred-height;
            list := VerticalLayout {
                alignment: start;
                for e in root.entries: LogRow { e: e; }
            }
        }
    }
}
```

If a property name above doesn't compile (Slint version differences), fix it to the nearest equivalent. Report each such change.

- [ ] **Step 5: Wire `app.slint` with the Edit tool** (gotcha #1 and #2):
  - Add to the imports: `import { CommandLogPane, UiLogEntry } from "command_log.slint";`. Also add `UiLogEntry` to the `export { … }` list if `app.slint` re-exports structs for Rust. Check how `ScanItem` / `ChartState` reach Rust and mirror that.
  - Add to the AppWindow properties: `in property <[UiLogEntry]> command-log;`, `callback command-log-open();` and `callback command-log-clear();`.
  - In `changed selection => { … }`, add `if root.selection == Pane.command-log { root.command-log-open(); }`.
  - Replace the sidebar row (currently lines 357–360) with:
    ```slint
                    SidebarRow {
                        label: "Command Log"; glyph: "\u{E756}"; tile: #5e5ce6;
                        text-color: Theme.text-primary;
                        selected: root.selection == Pane.command-log;
                        clicked => { root.selection = Pane.command-log; }
                    }
    ```
  - Add `&& root.selection != Pane.command-log` to the "Coming soon." catch-all condition (line 413).
  - Add the pane after the General pane block:
    ```slint
            if root.selection == Pane.command-log: CommandLogPane {
                vertical-stretch: 1;
                entries: root.command-log;
                reload => { root.command-log-open(); }
                clear => { root.command-log-clear(); }
            }
    ```
  - **Report requirement:** after editing, Read the sidebar row lines and the catch-all line back and **paste them verbatim into the report**. The reviewer confirms them in the diff.

- [ ] **Step 6: Wire `main.rs`:** add `mod log_view;` and, next to `chart::install(&app);`, add `log_view::install(&app);`.
- [ ] **Step 7: Build and test.** Run `cargo build -p claude-dashboard`, `cargo test --workspace` and clippy. Expected: green.
- [ ] **Step 8: Run-check (smoke).** In PowerShell, set `$env:CLAUDE_DASHBOARD_SMOKE=1; $env:CLAUDE_DASHBOARD_FAKE_ROWS=1` and run `cargo run -p claude-dashboard`. Expected: the app exits on its own in about 1.2 s with no panic. Note in the report that clicking into the pane needs the controller run-check.
- [ ] **Step 9: Commit** with message `feat(windows): Command Log pane` and the trailer.

---

### Task 10: Run Command panel + entry points + delete hook (GUI)

**Files:**
- Create: `apps/windows/app/ui/run_command.slint`, `apps/windows/app/src/run_command.rs`
- Modify: `apps/windows/app/ui/components.slint` (AccountPane: replace the disabled `ActionButton { label: "Command Log"; enabled: false; }` with `ActionButton { label: "Run Command"; clicked => { root.run-command(); } }` and add `callback run-command();`; AccountCard: add a small terminal glyph button `\u{E756}` at the top-right with `callback run-command();`, its own `TouchArea` placed after the card's main one so it wins the click)
- Modify: `apps/windows/app/ui/app.slint` (run overlay; pass `run-command => { root.open-run-command(acct.id); }` from both the AccountCard loop and the AccountPane loop)
- Modify: `apps/windows/app/src/main.rs` (`mod run_command;`, `run_command::install(&app, nudge_tx.clone())`)
- Modify: `apps/windows/app/src/settings_accounts.rs` (`delete_account` also calls `run_commands::remove(account_id)` after a successful account removal, outside the store lock)
- Modify: `apps/windows/app/src/commands.rs` (remove the Task 8 `#[allow(dead_code)]`s)

**Interfaces:**
- Consumes: `commands::{execute, launch_in_terminal, classify}`, `runner::CancelToken`, `shell::detect`, `core::run_commands::{get, set, RunCommand}`, `core::settings::load`, `log_view::reload`.
- Produces:
  - Slint `export component RunCommandPanel` with `in property <string> account-name; in-out property <string> command; in property <bool> open-in-terminal; in property <bool> running; in property <[string]> lines; callback edited(string); callback toggle-terminal(bool); callback run(); callback cancel();`
  - AppWindow: `in-out property <bool> run-open; in property <string> run-account-name; in-out property <string> run-command-text; in property <bool> run-in-terminal; in property <bool> run-running; in property <[string]> run-lines; callback open-run-command(string); callback run-command-edited(string); callback run-terminal-toggled(bool); callback run-command-start(); callback run-command-cancel();`

Controller behaviour (`run_command.rs`, ports `RunCommandSheet.swift`). It keeps a `thread_local!` `RefCell<State>`: `account_id: String`, `user_touched: bool`, `generation: u64`, `cancel: Option<CancelToken>`, `debounce: slint::Timer`.

- **open(id):** look up the name in `account_rows`. On a worker, `run_commands::get(id)`, then on the UI thread set the text, set `run-in-terminal` from the saved toggle, `user_touched = false`, `run-lines = []`, `run-open = true`. If the text is non-empty, schedule a classify.
- **edited(text):** `user_touched = false`. Restart `debounce` single-shot for 350 ms. When it fires, `generation += 1` and capture `gen` and `text`. On a worker: `shell::detect(settings::load().shell.as_deref())`, then `commands::classify(&text, shell)`. Back on the UI thread, apply only if `gen == state.generation && !state.user_touched && text == app.run-command-text`: `run-in-terminal = (kind == Interactive)`.
- **toggle(on):** `user_touched = true`; set `run-in-terminal = on`.
- **start:** ignore a blank command or one already running. On a worker, save with `run_commands::set(id, &RunCommand { command, open_in_terminal })`.
  - Terminal: `commands::launch_in_terminal(.., Manual, shell)`, then on the UI thread `run-open = false`, nudge refresh, `log_view::reload`.
  - Otherwise: on the UI thread first set `run-running = true` and `run-lines = []`, and keep a fresh `CancelToken` in the state. On the worker, `commands::execute(.., Manual, shell, &token, &on_output)`. `on_output` splits the chunk on `\n`/`\r`, drops empty lines, and `invoke_from_event_loop` appends them, keeping only the **last 2** lines in `run-lines`. When done, on the UI thread: `run-running = false`, `run-open = false`, nudge refresh (`tx.send(())`), `log_view::reload`.
- **cancel:** if a token is running, `token.cancel()`. Set `run-open = false` either way. The worker still records the row as `Cancelled`; this mirrors macOS, where Cancel dismisses the sheet.
- Stop the debounce timer when the panel closes.

- [ ] **Step 1: Write `ui/run_command.slint`.** It is a modal overlay: a full-window scrim (`#00000066`) with a `TouchArea` that swallows clicks, and a centred 420 px card (`Theme.card-bg`, radius 10, border) containing:
  - title "Run Command" + `account-name` (secondary)
  - a `LineEdit` from `std-widgets.slint` bound two-way to `command`, with `edited(t) => { root.edited(t); }` and `accepted => { if root.command != "" && !root.running { root.run(); } }`
  - a `CheckBox { text: "Open in Terminal"; checked: root.open-in-terminal; toggled => { root.toggle-terminal(self.checked); } }`
  - the `lines` in monospace 11 px, elided
  - a right-aligned row with Cancel and a primary Run button (label `running ? "Running…" : "Run"`, disabled when the command is empty or a run is in progress)

  Give the `LineEdit` focus when the panel opens (`init => { le.focus(); }` on the conditional element).
- [ ] **Step 2: Wire `app.slint` with the Edit tool.** Import `RunCommandPanel`, add the properties and callbacks listed above, and place the overlay **last** in AppWindow so it draws on top:
  ```slint
    if root.run-open: RunCommandPanel {
        width: parent.width; height: parent.height;
        account-name: root.run-account-name;
        command <=> root.run-command-text;
        open-in-terminal: root.run-in-terminal;
        running: root.run-running;
        lines: root.run-lines;
        edited(t) => { root.run-command-edited(t); }
        toggle-terminal(on) => { root.run-terminal-toggled(on); }
        run => { root.run-command-start(); }
        cancel => { root.run-command-cancel(); }
    }
  ```
  Add `run-command => { root.open-run-command(acct.id); }` to the `AccountCard` instance in the Dashboard loop and to the `AccountPane` instance in the account loop. **Report requirement:** paste the edited `AccountPane` button line from `components.slint` and both `run-command =>` lines from `app.slint`, read back verbatim.
- [ ] **Step 3: Implement `run_command.rs`** per the controller behaviour above, with `pub fn install(app: &AppWindow, nudge: std::sync::mpsc::Sender<()>)`. The session key is never touched here.
- [ ] **Step 4: Add the delete hook test** in `settings_accounts.rs` tests, following the existing pattern (testenv lock + temp APPDATA):
  ```rust
    #[test]
    fn delete_drops_the_saved_run_command() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        store::save_accounts(&[account("a"), account("b")]).unwrap();
        use claude_dashboard_core::run_commands::{get, set, RunCommand};
        set("a", &RunCommand { command: "ccbf".into(), open_in_terminal: false }).unwrap();
        set("b", &RunCommand { command: "echo".into(), open_in_terminal: false }).unwrap();
        delete_account("a").unwrap();
        assert!(get("a").is_none());
        assert!(get("b").is_some(), "other account untouched");
    }
  ```
  Run it and watch it fail (`get("a")` still `Some`). Then add `if let Err(e) = claude_dashboard_core::run_commands::remove(account_id) { eprintln!("drop run command failed: {e}"); }` right after the successful `store::save_accounts`, inside `delete_account` (still under the lock is fine, since it's a different file with its own mutex). Run it and watch it pass. If the existing test helper `account(id)` has a different name, use it.
- [ ] **Step 5: Build and test.** Run `cargo build -p claude-dashboard`, `cargo test --workspace` and clippy (no `dead_code` allowances left in `commands.rs`).
- [ ] **Step 6: Run-check (smoke)** with `CLAUDE_DASHBOARD_SMOKE=1 CLAUDE_DASHBOARD_FAKE_ROWS=1`: exits cleanly. Record in the report that the interactive flow (open panel → type `echo hi` → Run → row appears in Command Log; type `htop`/`claude` → toggle flips to Terminal → opens Windows Terminal) needs the controller run-check.
- [ ] **Step 7: Commit** with message `feat(windows): Run Command panel and saved commands` and the trailer.

---

### Task 11: Auto-run in the refresh loop, Claude Code badge, shell picker (GUI + wiring)

**Files:**
- Modify: `apps/windows/app/src/refresh.rs` (`refresh_once` reads `claude_code::active_email()` once per cycle and passes `active_claude_code_email: email.as_deref()`)
- Modify: `apps/windows/app/src/main.rs` (`spawn_refresh_loop`: an `AutoRunLatch` and dispatch; `fake_rows`: mark bob `is_active_claude_code: true`)
- Modify: `apps/windows/app/ui/theme.slint` (`UiRow` + `claude-code: bool`), `apps/windows/app/src/model.rs` (`claude_code: row.is_active_claude_code`)
- Modify: `apps/windows/app/ui/components.slint` (an 8 px green dot `Theme.ok` after the name in `AccountCard` and `AccountPane` when `data.claude-code`)
- Modify: `apps/windows/app/src/settings_general.rs` (shell list/current/set) and `apps/windows/app/ui/app.slint` (`GeneralSettingsPane` "Commands" group with shell chips; AppWindow `in property <[UiShell]> shells; in-out property <string> shell-key; callback set-shell(string);`, `export struct UiShell { key: string, label: string }` in app.slint or theme.slint)

**Interfaces:**
- Consumes: `core::auto_run::AutoRunLatch`, `core::run_commands::load`, `core::claude_code::active_email`, `commands::execute`, `runner::CancelToken`, `shell::{detect, detect_available, ShellKind}`, `log_view::reload`.
- Produces:
  - `settings_general::shells() -> Vec<(String, String)>` (key, label) for available shells
  - `settings_general::current_shell_key() -> String` (`shell::detect(settings.shell)` → `kind.setting_key()`, or `""` if none)
  - `settings_general::set_shell(key: &str) -> Result<(), String>` (an unknown key is rejected with `Err`)
  - `fn dispatch_auto_runs(due: Vec<String>, commands: &HashMap<String, RunCommand>, weak: slint::Weak<AppWindow>)` in `main.rs`

Refresh-loop change (inside the `loop` in `spawn_refresh_loop`, after `prev = out.rows.clone();`):

```rust
            let saved = claude_dashboard_core::run_commands::load();
            let due = latch.due(&out.rows, |id| {
                saved.get(id).is_some_and(|c| !c.command.trim().is_empty())
            });
            dispatch_auto_runs(due, &saved, weak.clone());
```

`dispatch_auto_runs` spawns one thread per due id. Each thread runs `commands::execute(&cmd, Some(&id), CommandTrigger::AutoReset, shell.as_ref(), &CancelToken::new(), &|_| {})`, with `shell = shell::detect(settings::load().shell.as_deref())`, then calls `log_view::reload(&weak)`. Runs are always hidden (Ruling 2). `latch` is a `let mut latch = AutoRunLatch::new();` before the loop. The refresh loop never waits on a command.

- [ ] **Step 1: Write the failing tests** in `settings_general.rs`:
  ```rust
    #[test]
    fn set_shell_persists_known_keys_only() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        set_shell("cmd").unwrap();
        assert_eq!(settings::load().shell.as_deref(), Some("cmd"));
        assert!(set_shell("zsh").is_err());
        assert_eq!(settings::load().shell.as_deref(), Some("cmd"));
        assert_eq!(current_shell_key(), "cmd", "cmd.exe always exists on Windows");
    }
  ```
  And in `model.rs` tests:
  ```rust
    #[test]
    fn claude_code_flag_maps_to_ui() {
        let mut r = row();
        assert!(!to_ui_row(&r, 1e9).claude_code);
        r.is_active_claude_code = true;
        assert!(to_ui_row(&r, 1e9).claude_code);
    }
  ```
  Run them and watch them fail.
- [ ] **Step 2: Implement** the `settings_general` functions, the `UiRow` field and mapping, and the `refresh_once` email pass-through.
- [ ] **Step 3: Implement the loop change** and `dispatch_auto_runs` as specified.
- [ ] **Step 4: UI.** Add the dot to the card and pane next to the name. In `GeneralSettingsPane`, add a "Commands" `GroupHeader` + `SettingsCard` with the text "Run commands in", a row of `IntervalChip`s (`for s in root.shells: IntervalChip { label: s.label; active: root.shell-key == s.key; clicked => { root.set-shell(s.key); } }`), and the caption "Started with your profile, so functions and aliases resolve." (secondary, 11 px). Wire the new pane props through AppWindow. In `main.rs`, load on a worker at startup (`shells()` + `current_shell_key()` → `invoke_from_event_loop` sets `shells`, `shell-key`). `on_set_shell(key)` saves on a worker, then re-reads `current_shell_key()` back into `shell-key` (the same shape as `on_set_launch_at_startup`).
- [ ] **Step 5: Build, test, clippy.** Run `cargo test --workspace`. Expected: green.
- [ ] **Step 6: Run-check (screenshot).** Run with `CLAUDE_DASHBOARD_FAKE_ROWS=1` (no SMOKE), screenshot per the HANDOFF §3 recipe, then `Stop-Process`. Expected: bob's card shows the green dot. Report the screenshot path. The General pane needs a click, so leave it to the controller run-check.
- [ ] **Step 7: Commit** with message `feat(windows): auto-run on reset, Claude Code badge, shell picker` and the trailer.

---

### Task 12: Help pane (GUI) — enables Tools › Help

**Files:**
- Create: `apps/windows/app/ui/help.slint`
- Modify: `apps/windows/app/ui/app.slint` (import; sidebar Help row lines 361–364; catch-all condition; pane)

**Interfaces:**
- Produces: Slint `export component HelpPane inherits Flickable` (no properties).

Content follows the section structure of `HelpView.swift`, rewritten for Windows. Each section is a `GroupHeader`-style title plus `SettingsCard`-style body `Text`s with `wrap: word-wrap`. Copy:

1. **Getting Started.** "Claude Dashboard shows your Claude.ai usage across several accounts from the system tray. Add an account from Settings › Accounts › Add Account. The browser extension is the easiest way: install it once per browser profile and it keeps the session key current. You can also scan a browser profile (Chrome may block this; the extension always works) or paste a session key."
2. **Reading Your Usage.** "Each card shows the 5-hour and 7-day windows, plus Fable where your plan has it. Color shows how much is left: green is plenty, yellow is halfway, red is nearly out. The countdown shows when the window resets. The animal shows your burn rate: a sloth is slow, a cheetah is fast. A green dot next to a name marks the account Claude Code is signed in to."
3. **Dashboard and Charts.** "Click a card to open the account. View chart shows its history; drag to zoom, double-click to reset. Overview compares all accounts. Left-click the tray icon for a quick summary; right-click it for Open Dashboard, Refresh and Quit. Closing the window keeps the app running in the tray."
4. **Commands.** "Run Command (the terminal button on a card, or on the account page) saves one command per account and runs it. Programs that need a terminal, like claude, vim or ssh, open in Windows Terminal; others run hidden, and their output goes to the Command Log. A hidden run stops after 60 seconds. The saved command also runs on its own when a usage window resets. Choose the shell under Settings › General › Commands. It starts with your profile, so your functions and aliases work."
5. **Troubleshooting.** "An account marked Expired needs a fresh key: open claude.ai in that browser (extension) or paste the key again. If the extension shows a red !, the helper is missing or reported an error; reinstall the app. If scanning says the cookie is app-bound, use the extension instead."

- [ ] **Step 1: Write `ui/help.slint`** with the five sections. Reuse `GroupHeader` / `SettingsCard` if `app.slint` exports them. Otherwise define local equivalents in `help.slint`; never import from `app.slint`, which would be circular.
- [ ] **Step 2: Wire `app.slint` with the Edit tool.** Enable the Help row exactly as in Task 9, with `Pane.help`. Add `&& root.selection != Pane.help` to the catch-all, so once both are in, **no pane falls through to "Coming soon."**. Add `if root.selection == Pane.help: HelpPane { vertical-stretch: 1; }`. If the catch-all `Text` is now unreachable for every `Pane` value, delete that block instead of extending it. **Report requirement:** paste the Help sidebar row and the catch-all (or confirm its deletion) verbatim.
- [ ] **Step 3: Build, test, clippy, smoke run.** Expected: green, clean self-quit.
- [ ] **Step 4: Commit** with message `feat(windows): Help pane` and the trailer.

---

### Task 13: Contract, CLAUDE.md, final checks

**Files:**
- Modify: `contract/windows.md`, `CLAUDE.md`

- [ ] **Step 1: `contract/windows.md`.** Add two rows to the Data paths table: `Command log DB | %LOCALAPPDATA%\claude-dashboard\command_logs.db` and `Saved run commands | %APPDATA%\claude-dashboard\run-commands.json`. Add a `## Command Log` section covering:
  - the trigger and status raw values and labels (from Global Constraints), noting that `autoEmpty` is recorded vocabulary only and never fired (Ruling 1)
  - the 500-row cap by id and the 4096-byte tail
  - the auto-run rule (usage present and the 5h or 7d `resets_at` missing; once per episode; re-armed when both windows report a reset; only for accounts with a saved, non-blank command; always hidden)
  - the `run-commands.json` shape and delete semantics (Ruling 4)
  - the `settings.json` `shell` key and its values, plus the auto default order
  - the profile-loading invocations per shell
  - the Job Object contract (`KILL_ON_JOB_CLOSE`, assigned before the child runs, timeout 60 s, cancel, background leftovers ended when the shell exits — Ruling 3, stdin NUL)
  - interactive launch (`wt.exe new-tab … --`, `;` escaped; fallback `conhost.exe`)
  - the active Claude Code account (`%USERPROFILE%\.claude.json` → `oauthAccount.emailAddress`, exact match; sort tier only when nothing is pinned)
- [ ] **Step 2: `CLAUDE.md`.** In the Windows section, extend the `apps/windows/app/` bullet with a "Sub-project 5 adds …" sentence: Command Log pane, Run Command panel, auto-run on reset, shell picker, Claude Code badge, Help. Add a bullet **Command `core` modules:** `command_log`, `command_classifier`, `auto_run`, `claude_code`, `run_commands`. Add a bullet **App process control:** `shell.rs`, `terminal.rs`, `runner.rs` (Job Object), `commands.rs`. Keep the existing style: one dense paragraph per bullet.
- [ ] **Step 3: Full verification.** Run `cd apps/linux && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`, then `cd apps/windows && cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo build -p claude-dashboard`, then `cd apps/windows/extension && node --test`. Expected: all green. Paste the summary lines into the report.
- [ ] **Step 4: BOM check.** For every file this sub-project created or modified, `head -c 3 <file> | od -An -tx1` must not print `ef bb bf`.
- [ ] **Step 5: Commit** with message `docs(windows): command log contract and CLAUDE.md` and the trailer.

---

## Controller run-check (after Task 13, before the final review)

Clicks can't be scripted with the screenshot recipe, so ask the user (or do it interactively if possible) to verify:
1. Tools › Command Log opens; it is empty, then shows rows after a run; Clear asks first.
2. On an account page, Run Command opens the panel with the saved command pre-filled. `echo hi` → Run → the panel closes and a row with `exit 0` appears.
3. Typing `claude` flips "Open in Terminal" on after about 350 ms (plus profile load time); Run opens a Windows Terminal tab and a row reads "In Terminal".
4. `ping -n 100 127.0.0.1` → Run → Cancel: the row reads "Cancelled", and Task Manager shows no leftover `PING.EXE`.
5. Settings › General › Commands lists only the installed shells, and the choice persists across a restart.
6. Help opens; nothing in the sidebar says "Coming soon".
