//! Dispatch smoke test for `claude-dashboard-helper`.
//!
//! Per `contract/helper-cli.md`, "Dispatch": an unrecognized subcommand prints
//! `Unknown command: <command>\n` to stderr and exits 1; no subcommand at all
//! prints the usage banner to stderr and exits 1. Neither path touches the
//! network or a browser, so this runs anywhere `cargo test` runs.
//!
//! The banner is pinned byte for byte because it is the same text on macOS.
//! Asserting a Linux-flavoured wording here is what let the two drift apart
//! unnoticed until 2026-09-06.

use std::process::Command;

/// Byte-for-byte the banner in `contract/helper-cli.md`: seven lines, one
/// trailing newline, no trailing blank line.
const USAGE_BANNER: &str = "\
Usage: claude-dashboard-helper <command>

Commands:
  decrypt    Decrypt accounts and output JSON to stdout
  sync       Scan installed browsers for Claude sessions and save to accounts
  usage      Fetch usage JSON for an account (args: <orgId> <sessionKey>)
  add-key    Add or repair one account from a session key on stdin
";

fn helper() -> &'static str {
    env!("CARGO_BIN_EXE_claude-dashboard-helper")
}

#[test]
fn unknown_subcommand_prints_error_and_exits_1() {
    let out = Command::new(helper()).args(["bogus"]).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Unknown command: bogus\n"
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn no_args_prints_usage_banner_and_exits_1() {
    let out = Command::new(helper()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stderr), USAGE_BANNER);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn add_key_with_empty_stdin_reports_and_exits_1() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(helper())
        .args(["add-key"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"   \n").unwrap();
    let out = child.wait_with_output().unwrap();

    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "No session key on stdin.\n"
    );
    assert_eq!(out.status.code(), Some(1));
}
