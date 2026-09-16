//! `list` and `remove` — the two Linux-only commands an account-management UI
//! needs and `decrypt` deliberately cannot provide.
//!
//! Both are driven against a throwaway `XDG_CONFIG_HOME` so nothing here can
//! read or write the developer's real account store.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn helper() -> &'static str {
    env!("CARGO_BIN_EXE_claude-dashboard-helper")
}

struct Store {
    dir: PathBuf,
}

impl Store {
    fn new(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("cd-list-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("claude-dashboard")).unwrap();
        Store { dir }
    }

    fn write(&self, json: &str) {
        fs::write(self.dir.join("claude-dashboard/accounts.json"), json).unwrap();
    }

    fn read(&self) -> String {
        fs::read_to_string(self.dir.join("claude-dashboard/accounts.json")).unwrap()
    }

    fn run(&self, args: &[&str]) -> (String, String, i32) {
        let out = Command::new(helper())
            .args(args)
            .env("XDG_CONFIG_HOME", &self.dir)
            .output()
            .unwrap();
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
            out.status.code().unwrap_or(-1),
        )
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Two accounts: one active, one expired with no orgId. `decrypt` would show
/// only the first; `list` must show both, because the expired one is exactly
/// the row a management UI exists to repair.
const TWO_ACCOUNTS: &str = r#"[
  {"id":"A","name":"a@example.com","email":"a@example.com",
   "chromeProfilePath":"/p/a","chromeProfileName":"Profile 1",
   "orgId":"org-a","sessionKey":"cipher-a","browser":"chrome",
   "plan":"Max 5x","status":"active","isPinned":true,"source":"browser"},
  {"id":"B","name":"b@example.com","email":"b@example.com",
   "chromeProfilePath":"/p/b","browser":"brave",
   "plan":"Pro","status":"expired","source":"manual"}
]"#;

#[test]
fn list_includes_accounts_decrypt_would_filter_out() {
    let store = Store::new("filter");
    store.write(TWO_ACCOUNTS);

    let (stdout, _, code) = store.run(&["list"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let rows = parsed.as_array().unwrap();
    assert_eq!(rows.len(), 2, "expired accounts must be listed");

    // decrypt's inclusion filter is status == active && orgId.is_some(); the
    // second account fails both halves and is still here.
    assert_eq!(rows[1]["status"], "expired");
    assert_eq!(rows[1]["orgId"], serde_json::Value::Null);
}

#[test]
fn list_never_emits_a_session_key() {
    let store = Store::new("secret");
    store.write(TWO_ACCOUNTS);

    let (stdout, _, _) = store.run(&["list"]);
    assert!(
        !stdout.contains("sessionKey"),
        "list must not carry a sessionKey field"
    );
    assert!(
        !stdout.contains("cipher-a"),
        "list must not carry a session key value, encrypted or not"
    );
}

#[test]
fn list_carries_the_fields_an_account_ui_needs() {
    let store = Store::new("fields");
    store.write(TWO_ACCOUNTS);

    let (stdout, _, _) = store.run(&["list"]);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let first = &parsed.as_array().unwrap()[0];

    assert_eq!(first["id"], "A");
    assert_eq!(first["email"], "a@example.com");
    assert_eq!(first["plan"], "Max 5x");
    assert_eq!(first["status"], "active");
    assert_eq!(first["isPinned"], true);
    // source and chromeProfileName are what the expired-card guidance branches
    // on: "you pasted this key by hand" vs "open browser profile X".
    assert_eq!(first["source"], "browser");
    assert_eq!(first["chromeProfileName"], "Profile 1");
    assert_eq!(parsed.as_array().unwrap()[1]["source"], "manual");
}

#[test]
fn list_of_an_empty_store_is_an_empty_array_not_an_error() {
    // Unlike decrypt, "you have no accounts" is the answer the caller asked
    // for, not a failure.
    let store = Store::new("empty");
    store.write("[]");

    let (stdout, _, code) = store.run(&["list"]);
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "[]");
}

#[test]
fn remove_deletes_only_the_named_account() {
    let store = Store::new("remove");
    store.write(TWO_ACCOUNTS);

    let (_, _, code) = store.run(&["remove", "A"]);
    assert_eq!(code, 0);

    let remaining: serde_json::Value = serde_json::from_str(&store.read()).unwrap();
    let rows = remaining.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "B");
}

#[test]
fn remove_of_a_missing_id_fails_rather_than_succeeding_silently() {
    let store = Store::new("missing");
    store.write(TWO_ACCOUNTS);

    let (_, stderr, code) = store.run(&["remove", "nope"]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "No account with id nope.\n");
    // The store is untouched.
    let remaining: serde_json::Value = serde_json::from_str(&store.read()).unwrap();
    assert_eq!(remaining.as_array().unwrap().len(), 2);
}

#[test]
fn remove_without_an_id_reports_its_usage() {
    let store = Store::new("usage");
    store.write(TWO_ACCOUNTS);

    let (_, stderr, code) = store.run(&["remove"]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "Usage: claude-dashboard-helper remove <id>\n");
}
