use claude_dashboard_core::burn_rate::{needs_fast_refresh, BurnRateResult, BurnRateTracker};
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

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn burn_rate_rocket_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/burn-rate-rocket.json")).unwrap();
    assert!(!cases.is_empty());
    for c in &cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let r = BurnRateResult::from_rate(
            c["rate_pct_per_min"].as_f64().unwrap(),
            c["sustained_seconds"].as_f64().unwrap(),
            c["utilization"].as_f64().unwrap(),
        );
        assert_eq!(r.level, c["expect_level"].as_u64().unwrap() as u8, "level - {name}");
        assert_eq!(r.animal, c["expect_animal"].as_str().unwrap(), "animal - {name}");
        let want = c["expect_projected_seconds"].as_f64().unwrap();
        assert!(close(r.projected_time, want), "projected {} vs {want} - {name}", r.projected_time);
    }
}

#[test]
fn burn_rate_tracker_cases() {
    let file: Value = serde_json::from_str(&contract("cases/burn-rate-tracker.json")).unwrap();
    let cases = file["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for c in cases {
        let name = c["name"].as_str().unwrap_or("<unnamed>");
        let mut tracker = BurnRateTracker::default();
        for (i, s) in c["steps"].as_array().unwrap().iter().enumerate() {
            let got = tracker.record(
                "a",
                0,
                s["utilization"].as_f64().unwrap(),
                s["resets_at"].as_f64(),
                s["at"].as_f64().unwrap(),
            );
            match s["expect_level"].as_u64() {
                None => assert!(got.is_none(), "step {i} expected none, got {got:?} - {name}"),
                Some(level) => {
                    let r = got.unwrap_or_else(|| panic!("step {i} expected a result - {name}"));
                    assert_eq!(r.level, level as u8, "step {i} level - {name}");
                    let want = s["expect_projected_seconds"].as_f64().unwrap();
                    assert!(close(r.projected_time, want), "step {i} projected {} vs {want} - {name}", r.projected_time);
                }
            }
        }
    }
}

#[test]
fn fast_refresh_cases() {
    let cases: Vec<Value> = serde_json::from_str(&contract("cases/fast-refresh.json")).unwrap();
    assert!(!cases.is_empty());
    for c in &cases {
        let got = needs_fast_refresh(
            c["five_hour_level"].as_u64().map(|v| v as u8),
            c["is_active"].as_bool().unwrap(),
            c["has_error"].as_bool().unwrap(),
            c["auto_refresh_enabled"].as_bool().unwrap(),
        );
        assert_eq!(got, c["expect"].as_bool().unwrap(), "{}", c["name"].as_str().unwrap());
    }
}
