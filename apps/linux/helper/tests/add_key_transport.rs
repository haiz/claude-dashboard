//! `add-key` driven as a real process: stdin, two endpoints, and a store write.
//! `contract/helper-cli.md` "add-key" is the source of truth for every string,
//! and `contract/cases/manual-key.json` for what the repair branch may write.

mod support;

use claude_dashboard_core::api::BASE_URL_OVERRIDE_VAR;
use support::{run_helper, LoopbackServer, Response};

/// The store lives at $XDG_CONFIG_HOME/claude-dashboard/accounts.json.
fn seed_store(dir: &std::path::Path, json: &str) {
    let cfg = dir.join("claude-dashboard");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("accounts.json"), json).unwrap();
}

fn read_store(dir: &std::path::Path) -> serde_json::Value {
    let path = dir.join("claude-dashboard").join("accounts.json");
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap(),
        Err(_) => serde_json::json!([]),
    }
}

fn run_add_key(
    server: &LoopbackServer,
    dir: &std::path::Path,
    stdin: &str,
) -> support::Output {
    // Bound to a local: `&server.origin()` is a `&String` where `&str` is wanted.
    let origin = server.origin();
    run_helper(
        &["add-key"],
        Some(stdin),
        &[
            (BASE_URL_OVERRIDE_VAR, origin.as_str()),
            ("XDG_CONFIG_HOME", dir.to_str().unwrap()),
            ("XDG_DATA_HOME", dir.to_str().unwrap()),
        ],
    )
}

fn account_body(uuid: &str, email: Option<&str>, org: &str, capabilities: &str) -> String {
    let email_field = email.map(|e| format!(r#""email_address":"{e}","#)).unwrap_or_default();
    format!(
        r#"{{{email_field}"uuid":"{uuid}","memberships":[
             {{"role":"admin","organization":{{"uuid":"{org}","name":"Org","capabilities":{capabilities}}}}}
           ]}}"#
    )
}

fn orgs_body(org: &str, capabilities: &str) -> String {
    format!(r#"[{{"uuid":"{org}","name":"Org","capabilities":{capabilities}}}]"#)
}

#[test]
fn empty_stdin_is_rejected_before_any_request() {
    let server = LoopbackServer::start();
    let d = tempfile::tempdir().unwrap();

    let out = run_add_key(&server, d.path(), "   \n");

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "No session key on stdin.\n");
    assert!(server.recorded().is_empty(), "nothing should reach the network");
}

#[test]
fn account_endpoint_rejecting_the_key_is_reported() {
    let server = LoopbackServer::start();
    let d = tempfile::tempdir().unwrap();
    server.respond("/api/account", Response::status(401));

    let out = run_add_key(&server, d.path(), "sk-fake-expired-key\n");

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "Session key not accepted (expired or invalid).\n");
    assert_eq!(read_store(d.path()), serde_json::json!([]));
}

#[test]
fn add_without_a_chat_org_is_rejected() {
    let server = LoopbackServer::start();
    let d = tempfile::tempdir().unwrap();
    server.respond(
        "/api/account",
        Response::json(&account_body("acct-1", Some("me@example.com"), "org-1", r#"["raven"]"#)),
    );
    server.respond("/api/organizations", Response::json(&orgs_body("org-1", r#"["raven"]"#)));

    let out = run_add_key(&server, d.path(), "sk-fake-test-key\n");

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "No organization with chat access.\n");
    assert_eq!(read_store(d.path()), serde_json::json!([]));
}

#[test]
fn add_stores_the_account_with_an_encrypted_key() {
    let key = "sk-fake-test-key";
    let server = LoopbackServer::start();
    let d = tempfile::tempdir().unwrap();
    server.respond(
        "/api/account",
        Response::json(&account_body("acct-1", Some("me@example.com"), "org-1", r#"["chat"]"#)),
    );
    server.respond(
        "/api/organizations",
        Response::json(&orgs_body("org-1", r#"["chat","claude_pro"]"#)),
    );

    let out = run_add_key(&server, d.path(), &format!("{key}\n"));

    assert_eq!(out.code, 0);
    assert_eq!(out.stderr, "Added: me@example.com (Pro)\n");
    assert!(!out.stderr.contains(key), "the session key must never reach stderr");

    let stored = read_store(d.path());
    assert_eq!(stored.as_array().unwrap().len(), 1);
    assert_eq!(stored[0]["accountUuid"], "acct-1");
    assert_eq!(stored[0]["orgId"], "org-1");
    assert_eq!(stored[0]["plan"], "Pro");
    assert_eq!(stored[0]["source"], "manual");
    assert_eq!(stored[0]["status"], "active");
    assert_ne!(stored[0]["sessionKey"], key, "the key must be stored encrypted");

    let recorded = server.recorded();
    assert_eq!(recorded.len(), 2);
    for request in recorded {
        assert_eq!(request.headers["cookie"], format!("sessionKey={key}"));
    }
}

#[test]
fn repair_rewrites_only_the_permitted_fields() {
    let key = "sk-fake-new-key";
    let server = LoopbackServer::start();
    let d = tempfile::tempdir().unwrap();
    seed_store(
        d.path(),
        r#"[{"id":"A","name":"me@example.com","email":"me@example.com",
             "chromeProfilePath":"/p","orgId":"org-1","accountUuid":"acct-1",
             "sessionKey":"OLD-STORED-VALUE","browser":"chrome","plan":"Pro",
             "status":"active","source":"browser"}]"#,
    );
    server.respond(
        "/api/account",
        Response::json(&account_body("acct-1", Some("me@example.com"), "org-1", r#"["chat"]"#)),
    );
    server.respond(
        "/api/organizations",
        Response::json(&orgs_body("org-1", r#"["chat","claude_pro"]"#)),
    );

    let out = run_add_key(&server, d.path(), &format!("{key}\n"));

    assert_eq!(out.code, 0);
    assert_eq!(out.stderr, "Updated key: me@example.com\n");
    assert!(!out.stderr.contains(key), "the session key must never reach stderr");

    let stored = read_store(d.path());
    assert_eq!(stored.as_array().unwrap().len(), 1, "a repair must not add a record");
    // Differs from both the old stored value and the plaintext.
    assert_ne!(stored[0]["sessionKey"], "OLD-STORED-VALUE");
    assert_ne!(stored[0]["sessionKey"], key);
    assert_eq!(stored[0]["id"], "A");
    assert_eq!(stored[0]["orgId"], "org-1");
    assert_eq!(stored[0]["source"], "browser", "a key never changes a record's source");
    assert_eq!(stored[0]["plan"], "Pro");
}

#[test]
fn repair_reports_a_changed_plan() {
    let server = LoopbackServer::start();
    let d = tempfile::tempdir().unwrap();
    seed_store(
        d.path(),
        r#"[{"id":"A","name":"me@example.com","email":"me@example.com",
             "chromeProfilePath":"/p","orgId":"org-1","accountUuid":"acct-1",
             "sessionKey":"OLD-STORED-VALUE","browser":"chrome","plan":"Pro",
             "status":"active","source":"browser"}]"#,
    );
    server.respond(
        "/api/account",
        Response::json(&account_body("acct-1", Some("me@example.com"), "org-1", r#"["chat"]"#)),
    );
    // claude_max without claude_pro resolves to the generic Max tier.
    server.respond(
        "/api/organizations",
        Response::json(&orgs_body("org-1", r#"["chat","claude_max"]"#)),
    );

    let out = run_add_key(&server, d.path(), "sk-fake-new-key\n");

    assert_eq!(out.code, 0);
    assert_eq!(
        out.stderr,
        "Updated key: me@example.com\nUpdated plan: me@example.com (Pro -> Max)\n"
    );
    assert_eq!(read_store(d.path())[0]["plan"], "Max");
}
