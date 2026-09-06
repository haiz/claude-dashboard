//! Golden tests for `claude-dashboard-helper decrypt` against
//! `contract/helper-cli.md` "decrypt": exact stderr text, exit codes, and
//! sorted-key JSON output.

use std::process::Command;

fn helper() -> &'static str {
    env!("CARGO_BIN_EXE_claude-dashboard-helper")
}

fn run_in(dir: &std::path::Path, args: &[&str]) -> (String, String, i32) {
    let out = Command::new(helper())
        .args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("XDG_DATA_HOME", dir)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn no_accounts_file_message_and_exit() {
    let d = tempfile::tempdir().unwrap();
    let (_o, err, code) = run_in(d.path(), &["decrypt"]);
    assert_eq!(err, "No accounts found. Run: claude-dashboard-cli sync\n");
    assert_eq!(code, 1);
}

#[test]
fn active_with_org_is_projected_with_sorted_keys() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("claude-dashboard");
    std::fs::create_dir_all(&cfg).unwrap();
    // one active+orgId account (included), one expired (excluded)
    std::fs::write(
        cfg.join("accounts.json"),
        r#"[
      {"id":"A","name":"me","email":"me@x.com","chromeProfilePath":"/p",
       "orgId":"org-1","sessionKey":"PLAINTEXT-OR-CIPHER","browser":"chrome",
       "plan":"Pro","status":"active"},
      {"id":"B","name":"old","chromeProfilePath":"/q","orgId":"org-2",
       "plan":"Pro","status":"expired"}
    ]"#,
    )
    .unwrap();
    let (out, _e, code) = run_in(d.path(), &["decrypt"]);
    assert_eq!(code, 0);
    // exactly one element, keys alphabetical
    assert!(out.contains("\"email\""));
    let keys_order = out.find("\"email\"").unwrap() < out.find("\"name\"").unwrap()
        && out.find("\"name\"").unwrap() < out.find("\"orgId\"").unwrap()
        && out.find("\"orgId\"").unwrap() < out.find("\"plan\"").unwrap()
        && out.find("\"plan\"").unwrap() < out.find("\"sessionKey\"").unwrap()
        && out.find("\"sessionKey\"").unwrap() < out.find("\"status\"").unwrap();
    assert!(keys_order, "keys must be sorted: {out}");
    assert!(!out.contains("org-2"), "expired account must be excluded");
}

#[test]
fn stored_but_none_active_message() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("claude-dashboard");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("accounts.json"),
        r#"[{"id":"B","name":"old","chromeProfilePath":"/q","orgId":"org-2","plan":"Pro","status":"expired"}]"#,
    )
    .unwrap();
    let (_o, err, code) = run_in(d.path(), &["decrypt"]);
    assert_eq!(err, "No active accounts with session keys found.\n");
    assert_eq!(code, 1);
}

/// A stored value that is not a valid ciphertext comes back verbatim — the
/// command falls back to the stored string with no error path, matching
/// `apps/macos/Helper/DecryptCommand.swift`'s `?? encrypted`.
#[test]
fn undecryptable_session_key_is_passed_through_verbatim() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("claude-dashboard");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("accounts.json"),
        r#"[{"id":"A","name":"me","email":"me@x.com","chromeProfilePath":"/p",
             "orgId":"org-1","sessionKey":"NOT-A-CIPHERTEXT","browser":"chrome",
             "plan":"Pro","status":"active"}]"#,
    )
    .unwrap();

    let (out, _e, code) = run_in(d.path(), &["decrypt"]);

    assert_eq!(code, 0);
    assert!(
        out.contains("\"sessionKey\": \"NOT-A-CIPHERTEXT\""),
        "ciphertext must survive a failed decrypt: {out}"
    );
}

/// The projection's `BTreeMap` inserts every key unconditionally, so an
/// included account with no stored session key projects `sessionKey: null`
/// rather than omitting the key — the same six-key/null shape that
/// `apps/macos/Helper/DecryptCommand.swift`'s explicit `encode(to:)` was
/// fixed to match (see that file's commit history for the omission bug).
#[test]
fn included_account_with_no_session_key_projects_null() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("claude-dashboard");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("accounts.json"),
        r#"[{"id":"A","name":"me","chromeProfilePath":"/p",
             "orgId":"org-1","browser":"chrome",
             "plan":"Pro","status":"active"}]"#,
    )
    .unwrap();

    let (out, _e, code) = run_in(d.path(), &["decrypt"]);

    assert_eq!(code, 0);
    assert!(
        out.contains("\"sessionKey\": null"),
        "an absent session key must project as null, not be omitted: {out}"
    );
}
