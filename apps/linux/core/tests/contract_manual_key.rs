use claude_dashboard_core::identity::OrgMembership;
use claude_dashboard_core::manual_key::{manual_key_decision, ManualKeyDecision, StoredManualTarget};
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

fn memberships(v: &Value) -> Vec<OrgMembership> {
    v.as_array().unwrap_or(&vec![]).iter().map(|m| OrgMembership {
        uuid: m["uuid"].as_str().unwrap_or_default().to_string(),
        name: m["name"].as_str().unwrap_or_default().to_string(),
        capabilities: m["capabilities"].as_array().unwrap_or(&vec![])
            .iter().filter_map(|c| c.as_str().map(String::from)).collect(),
    }).collect()
}

#[test]
fn manual_key_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/manual-key.json")).unwrap();
    assert!(!cases.is_empty());

    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let stored = c["stored"].as_object().map(|s| StoredManualTarget {
            org_id: s["org_id"].as_str().map(String::from),
            account_uuid: s["account_uuid"].as_str().map(String::from),
            email: s["email"].as_str().map(String::from),
        });
        let fetched_uuid = c["fetched"]["uuid"].as_str().unwrap();
        let fetched_email = c["fetched"]["email"].as_str();
        let orgs = memberships(&c["memberships"]);

        let actual = manual_key_decision(stored.as_ref(), fetched_uuid, fetched_email, &orgs);

        let expect = &c["expect"];
        match expect["action"].as_str().unwrap() {
            "add" => {
                let want = expect["org_id"].as_str().unwrap();
                assert_eq!(actual, ManualKeyDecision::Add { org_id: want.to_string() },
                           "case: {name}");
            }
            "reject_no_chat_org" => {
                assert_eq!(actual, ManualKeyDecision::RejectNoChatOrg, "case: {name}");
            }
            "repair" => {
                let ManualKeyDecision::Repair { writes, warn_no_chat_org } = actual else {
                    panic!("case: {name} — expected a repair, got {actual:?}");
                };
                let want_warn = expect["warn_no_chat_org"]
                    .as_bool()
                    .unwrap_or_else(|| panic!("case: {name} — repair needs expect.warn_no_chat_org"));
                assert_eq!(warn_no_chat_org, want_warn, "case: {name} (warn_no_chat_org)");
                let w = &expect["writes"];
                assert_eq!(writes.org_id.as_deref(), w["org_id"].as_str(), "case: {name} (org_id)");
                assert_eq!(writes.account_uuid.as_deref(), w["account_uuid"].as_str(),
                           "case: {name} (account_uuid)");
                assert_eq!(writes.email.as_deref(), w["email"].as_str(), "case: {name} (email)");
            }
            other => panic!("case: {name} — unknown action {other}"),
        }
    }
}
