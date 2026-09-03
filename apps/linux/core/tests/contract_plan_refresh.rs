//! Drives `plan::refreshed_plan` from `contract/cases/plan-refresh.json` —
//! the same file `ClaudeDashboardTests/PlanRefreshContractTests.swift` reads.

use claude_dashboard_core::model::AccountPlan;
use claude_dashboard_core::plan::refreshed_plan;
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

/// A wire plan value (`"Pro"`, `"Max 5x"`, ...) as the case file writes it.
fn plan(v: &Value) -> Option<AccountPlan> {
    let wire = v.as_str()?;
    Some(serde_json::from_value(Value::String(wire.to_string()))
        .unwrap_or_else(|e| panic!("unknown plan wire value {wire:?}: {e}")))
}

fn wire(plan: Option<AccountPlan>) -> Option<String> {
    plan.map(|p| serde_json::to_value(p).unwrap().as_str().unwrap().to_string())
}

#[test]
fn plan_refresh_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/plan-refresh.json")).unwrap();
    assert!(!cases.is_empty(), "contract/cases/plan-refresh.json is empty");
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let stored = plan(&c["stored_plan"])
            .unwrap_or_else(|| panic!("case '{name}' has no `stored_plan`"));
        let hint = plan(&c["hint_plan"]); // absent/null -> None
        let expected = c.get("expect_write").and_then(Value::as_str);

        let actual = refreshed_plan(&stored, hint);

        assert_eq!(wire(actual).as_deref(), expected, "case: {name}");
    }
}
