//! End-to-end through the real binary: a framed message on stdin produces one
//! framed reply on stdout. The API base points at a closed port so
//! `fetch_account` fails fast and the reply is a deterministic `rejected`,
//! without needing a mock HTTP server. The happy path (a stored account) is
//! covered by the handler's unit tests against a fake Environment, and by the
//! manual smoke test in contract/windows.md.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn framed(body: &[u8]) -> Vec<u8> {
    [&(body.len() as u32).to_le_bytes()[..], body].concat()
}

fn read_framed(mut r: impl Read) -> Vec<u8> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).expect("reply length");
    let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
    r.read_exact(&mut body).expect("reply body");
    body
}

#[test]
fn a_framed_message_gets_a_framed_reply_and_writes_no_store() {
    let dir = tempfile::tempdir().unwrap();
    let message = serde_json::json!({
        "type": "sessionKey",
        "installId": "inst-roundtrip",
        "browser": "chrome",
        "sessionKey": "sk-ant-sid01-roundtrip",
    })
    .to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_claude-dashboard-bridge"))
        // Closed port: fetch_account fails fast -> identity None -> "rejected".
        .env("CLAUDE_DASHBOARD_API_BASE", "http://127.0.0.1:1")
        // Keep all store I/O inside the temp dir on every platform.
        .env("APPDATA", dir.path())
        .env("LOCALAPPDATA", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_DATA_HOME", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn bridge");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(&framed(message.as_bytes()))
        .unwrap();

    let reply_bytes = read_framed(child.stdout.take().unwrap());
    let status = child.wait().unwrap();
    assert!(status.success(), "bridge should exit 0 after replying");

    let reply: serde_json::Value = serde_json::from_slice(&reply_bytes).unwrap();
    assert_eq!(reply["ok"], serde_json::json!(false));
    assert_eq!(reply["error"], serde_json::json!("rejected"));
    // The key must never appear in the reply.
    assert!(!String::from_utf8_lossy(&reply_bytes).contains("sk-ant-sid01-roundtrip"));

    // A rejected key writes nothing.
    assert!(
        !dir.path().join("claude-dashboard").join("accounts.json").exists(),
        "a rejected key must not create the store"
    );
}

#[test]
fn empty_stdin_exits_cleanly_with_no_reply() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_claude-dashboard-bridge"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn bridge");
    // Close stdin immediately: a clean browser disconnect before any message.
    drop(child.stdin.take());

    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    let status = child.wait().unwrap();

    assert!(status.success(), "EOF before a message is a clean exit");
    assert!(out.is_empty(), "no reply when there was no message");
}
