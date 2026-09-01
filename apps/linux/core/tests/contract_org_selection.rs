use claude_dashboard_core::identity::{resolve_org_id, OrgMembership};
use serde_json::Value;
use std::path::PathBuf;

fn contract(rel: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")) // .../apps/linux/core
        .parent().unwrap()  // apps/linux
        .parent().unwrap()  // apps
        .parent().unwrap()  // repo root
        .join("contract").join(rel);
    std::fs::read_to_string(&root).unwrap_or_else(|e| panic!("read {root:?}: {e}"))
}

#[test]
fn org_selection_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/org-selection.json")).unwrap();
    assert!(!cases.is_empty());
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let last_active = c.get("last_active_org").and_then(Value::as_str);
        let memberships: Vec<OrgMembership> = c["memberships"].as_array().unwrap_or(&vec![])
            .iter()
            .map(|m| OrgMembership {
                uuid: m["uuid"].as_str().unwrap_or("").to_string(),
                name: m["name"].as_str().unwrap_or("").to_string(),
                capabilities: m.get("capabilities").and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
            })
            .collect();
        let expected = c.get("expect_org_id").and_then(Value::as_str);

        let actual = resolve_org_id(last_active, &memberships);

        assert_eq!(actual.as_deref(), expected, "case: {name}");
    }
}
