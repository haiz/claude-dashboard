use claude_dashboard_core::identity::{duplicate_index, is_duplicate, StoredIdentity};
use serde_json::Value;
use std::path::PathBuf;

fn contract(rel: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .parent().unwrap()
        .join("contract").join(rel);
    std::fs::read_to_string(&root).unwrap_or_else(|e| panic!("read {root:?}: {e}"))
}

#[test]
fn dedupe_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/dedupe.json")).unwrap();
    assert!(!cases.is_empty());
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let candidate_uuid = c["candidate"]["account_uuid"].as_str()
            .unwrap_or_else(|| panic!("case '{name}' needs candidate.account_uuid"));
        let candidate_email = c["candidate"].get("email").and_then(Value::as_str);
        let stored: Vec<StoredIdentity> = c["stored"].as_array().unwrap_or(&vec![])
            .iter()
            .map(|s| StoredIdentity {
                account_uuid: s.get("account_uuid").and_then(Value::as_str).map(String::from),
                email: s.get("email").and_then(Value::as_str).map(String::from),
            })
            .collect();
        let expected = c["expect_duplicate"].as_bool().unwrap_or(false);

        let actual = is_duplicate(candidate_uuid, candidate_email, &stored);

        assert_eq!(actual, expected, "case: {name}");

        // `sync`'s heal path needs *which* stored account matched, so the two
        // functions must never disagree about whether one did.
        let index = duplicate_index(candidate_uuid, candidate_email, &stored);
        assert_eq!(index.is_some(), expected, "case: {name} (duplicate_index)");
    }
}
