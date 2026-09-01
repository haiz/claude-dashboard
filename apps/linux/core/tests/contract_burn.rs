use claude_dashboard_core::burn_rate::BurnRateResult;
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
fn burn_rate_level_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/burn-rate-levels.json")).unwrap();
    assert!(!cases.is_empty());
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let secs = c["projected_seconds"].as_f64().unwrap();
        let r = BurnRateResult::from_projected_time(secs);
        assert_eq!(r.level, c["expect_level"].as_u64().unwrap() as u8, "level — {name}");
        assert_eq!(r.animal, c["expect_animal"].as_str().unwrap(), "animal — {name}");
        assert_eq!(r.projected_time, secs, "projected_time passthrough — {name}");
    }
}
