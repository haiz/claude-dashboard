//! Dispatch smoke test for `claude-dashboard-helper`.
//!
//! Per `contract/helper-cli.md` lines 9-14: an unrecognized subcommand prints
//! `Unknown command: <command>\n` to stderr and exits 1; no subcommand at all
//! prints the usage banner to stderr and exits 1. Neither path touches the
//! network or a browser, so this runs anywhere `cargo test` runs.

use std::process::Command;

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
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Usage: claude-dashboard-helper <decrypt|usage|sync>\n"
    );
    assert_eq!(out.status.code(), Some(1));
}
