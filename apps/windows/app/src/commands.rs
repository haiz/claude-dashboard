//! Runs a command, records it in the command log, hands it to a terminal, and
//! classifies it as interactive or not. The GUI calls this module.

use crate::runner::{self, CancelToken, RunOutcome};
use crate::shell::{self, ShellSpec};
use crate::terminal;
use claude_dashboard_core::command_classifier::{self, resolvable_token, CommandKind};
use claude_dashboard_core::command_log::{CommandLogStore, CommandStatus, CommandTrigger, NewEntry};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const RUN_TIMEOUT: Duration = Duration::from_secs(60);
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Store errors are logged, never surfaced.
fn record(e: &NewEntry) {
    match CommandLogStore::open() {
        Ok(store) => {
            if let Err(err) = store.record(e) {
                eprintln!("command log: record failed: {err:?}");
            }
        }
        Err(err) => eprintln!("command log: open failed: {err:?}"),
    }
}

/// Run `command` hidden, record one log row, and return the outcome.
pub fn execute(
    command: &str,
    account_id: Option<&str>,
    trigger: CommandTrigger,
    shell: Option<&ShellSpec>,
    cancel: &CancelToken,
    on_output: &(dyn Fn(&str) + Sync),
) -> RunOutcome {
    let outcome = match shell {
        Some(spec) => runner::run(
            &shell::run_invocation(spec, command),
            RUN_TIMEOUT,
            cancel,
            on_output,
        ),
        None => {
            let now = now_unix();
            RunOutcome {
                status: CommandStatus::LaunchFailed,
                exit_code: None,
                output_tail: "launch failed: no shell found".to_string(),
                started_unix: now,
                finished_unix: now,
            }
        }
    };
    record(&NewEntry {
        account_id,
        command,
        trigger,
        started_unix: outcome.started_unix,
        finished_unix: Some(outcome.finished_unix),
        status: outcome.status,
        exit_code: outcome.exit_code,
        output: Some(&outcome.output_tail),
    });
    outcome
}

/// Open `command` in a visible terminal and record the handoff.
pub fn launch_in_terminal(
    command: &str,
    account_id: Option<&str>,
    trigger: CommandTrigger,
    shell: Option<&ShellSpec>,
) -> CommandStatus {
    let started = now_unix();
    let result = match shell {
        Some(spec) => {
            let env = |k: &str| std::env::var(k).ok();
            let wt = terminal::wt_path(&env).filter(|p| p.is_file());
            let system_root = PathBuf::from(
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string()),
            );
            let inv = terminal::terminal_invocation(spec, command, wt.as_deref(), &system_root);
            terminal::launch(&inv)
        }
        None => Err("launch failed: no shell found".to_string()),
    };
    let (status, output) = match result {
        Ok(()) => (CommandStatus::LaunchedInTerminal, None),
        Err(msg) => (CommandStatus::LaunchFailed, Some(msg)),
    };
    record(&NewEntry {
        account_id,
        command,
        trigger,
        started_unix: started,
        finished_unix: Some(now_unix()),
        status,
        exit_code: None,
        output: output.as_deref(),
    });
    status
}

/// Classify by expanding the leading token through the shell (alias/function
/// bodies), then applying the shared classifier. The resolver run is not logged.
pub fn classify(command: &str, shell: Option<&ShellSpec>) -> CommandKind {
    let expanded = match (resolvable_token(command), shell) {
        (Some(tok), Some(spec)) => {
            let out = runner::run(
                &shell::resolve_invocation(spec, tok),
                RESOLVE_TIMEOUT,
                &CancelToken::new(),
                &|_| {},
            );
            if out.status == CommandStatus::Exited {
                out.output_tail
            } else {
                String::new()
            }
        }
        _ => String::new(),
    };
    command_classifier::classify(command, &expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellKind;
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
        // Metacharacters are never resolved, but classification still reads the
        // leading token, where `vim` is a whole word (same as macOS).
        assert_eq!(classify("$(vim)", Some(&cmd_shell())), CommandKind::Interactive);
        assert_eq!(classify("htop", None), CommandKind::Interactive);
    }
}
