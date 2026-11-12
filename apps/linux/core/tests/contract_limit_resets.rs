use base64::Engine;
use claude_dashboard_core::limit_resets::{field_name, parse_grants, summarize, ResetSummary};
use claude_dashboard_core::usage::parse_iso8601_to_unix_trunc;
use serde_json::Value;
use std::path::PathBuf;

fn contract(rel: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().parent().unwrap()
        .join("contract").join(rel);
    std::fs::read_to_string(&root).unwrap_or_else(|e| panic!("read {root:?}: {e}"))
}

fn check(name: &str, kind: &str, got: &Option<ResetSummary>, want: &Value) {
    match got {
        None => assert!(want.is_null(), "{name}: {kind} should be present"),
        Some(s) => {
            assert!(!want.is_null(), "{name}: {kind} should be absent");
            assert_eq!(s.count as u64, want["count"].as_u64().unwrap(), "{name}: {kind} count");
            assert_eq!(s.expires_at, want["expires_at_epoch"].as_i64().unwrap(), "{name}: {kind} expires_at");
            assert_eq!(s.urgent, want["urgent"].as_bool().unwrap(), "{name}: {kind} urgent");
        }
    }
}

#[test]
fn limit_resets_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/limit-resets.json")).unwrap();
    assert!(!cases.is_empty());
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let now = parse_iso8601_to_unix_trunc(c["now"].as_str().unwrap()).unwrap();
        let r = summarize(&parse_grants(&c["input"]), now);
        check(name, "full", &r.full, &c["expect"]["full"]);
        check(name, "fable", &r.fable, &c["expect"]["fable"]);
    }
}

#[test]
fn field_name_round_trips() {
    let n = field_name();
    assert!(n.len() == 11 && n.contains('_'));
    let enc = base64::engine::general_purpose::STANDARD.encode(n.as_bytes());
    assert_eq!(enc, "Y2VkYXJfZW1iZXI=");
}
