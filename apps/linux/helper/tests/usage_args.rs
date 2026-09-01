//! Arg-count contract test for `claude-dashboard-helper usage`.
//!
//! Per `contract/helper-cli.md` "usage": requires exactly two positional
//! arguments; fewer prints the usage banner to stderr and exits 1. Live
//! network paths (real HTTP success/error mapping) are covered by Task 12
//! acceptance, not here.

use std::process::Command;

fn helper() -> &'static str {
    env!("CARGO_BIN_EXE_claude-dashboard-helper")
}

#[test]
fn too_few_args_usage_message() {
    let out = Command::new(helper())
        .args(["usage", "only-one"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Usage: claude-dashboard-helper usage <orgId> <sessionKey>\n"
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn zero_args_usage_message() {
    let out = Command::new(helper()).args(["usage"]).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Usage: claude-dashboard-helper usage <orgId> <sessionKey>\n"
    );
    assert_eq!(out.status.code(), Some(1));
}
