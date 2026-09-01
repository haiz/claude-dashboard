use claude_dashboard_core::plan::detect_plan_tier;
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
fn plan_detection_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/plan-detection.json")).unwrap();
    assert!(!cases.is_empty());
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let org = &c["org"];
        let caps: Vec<String> = org.get("capabilities").and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let expected = c.get("expect_plan").and_then(Value::as_str); // absent/null -> None
        let actual = detect_plan_tier(org, &caps);
        let actual_wire = actual.map(|p| serde_json::to_value(p).unwrap()
            .as_str().unwrap().to_string());
        assert_eq!(actual_wire.as_deref(), expected, "case: {name}");
    }
}
