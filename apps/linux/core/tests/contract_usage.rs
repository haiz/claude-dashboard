use claude_dashboard_core::usage::UsageData;
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
fn usage_decoding_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/usage-decoding.json")).unwrap();
    assert!(!cases.is_empty(), "usage-decoding.json empty");
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let input = serde_json::to_string(&c["input"]).unwrap();
        let expect = &c["expect"];
        let decoded = UsageData::decode(&input);

        // None of the 7 cases currently in usage-decoding.json set
        // `expect.decodes: false` (verified by direct inspection of the file) —
        // this branch is defensive for a future case that does.
        if let Some(false) = expect.get("decodes").and_then(|v| v.as_bool()) {
            assert!(decoded.is_err(), "case '{name}' should fail to decode");
            continue;
        }
        let d = decoded.unwrap_or_else(|e| panic!("case '{name}': {e:?}"));
        assert_eq!(d.five_hour.utilization, expect["five_hour_utilization"].as_f64().unwrap(),
                   "five_hour_utilization — {name}");
        assert_eq!(d.five_hour.resets_at, expect["five_hour_resets_at_epoch"].as_i64(),
                   "five_hour_resets_at — {name}");
        assert_eq!(d.seven_day.utilization, expect["seven_day_utilization"].as_f64().unwrap(),
                   "seven_day_utilization — {name}");
        assert_eq!(d.seven_day.resets_at, expect["seven_day_resets_at_epoch"].as_i64(),
                   "seven_day_resets_at — {name}");
        assert_eq!(d.fable.is_some(), expect["fable_present"].as_bool().unwrap(),
                   "fable_present — {name}");
        if let Some(f) = &d.fable {
            assert_eq!(f.utilization, expect["fable_utilization"].as_f64().unwrap(),
                       "fable_utilization — {name}");
            assert_eq!(f.resets_at, expect["fable_resets_at_epoch"].as_i64(),
                       "fable_resets_at — {name}");
        }
    }
}
