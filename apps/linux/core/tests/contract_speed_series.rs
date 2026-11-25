//! Drives `contract/cases/speed-series.json` (README, "Speed series"), the fixture the
//! macOS speed-series tests read too, against `core::speed_series`. Fixture times are
//! seconds; the core works in milliseconds.

use claude_dashboard_core::chart::Entry;
use claude_dashboard_core::speed_series::{
    account_speed, max_factor, total_speed, AccountSpeed, SpeedPoint,
};
use serde_json::Value;
use std::path::PathBuf;

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")) // .../apps/linux/core
        .parent().unwrap()  // apps/linux
        .parent().unwrap()  // apps
        .parent().unwrap()  // repo root
        .join("contract/cases/speed-series.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn ms(v: &Value) -> f64 {
    v.as_f64().unwrap() * 1000.0
}

fn points(v: &Value) -> Vec<SpeedPoint> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| SpeedPoint { end_ms: ms(&p[0]), value: p[1].as_f64().unwrap() })
        .collect()
}

/// Compares `got` with the fixture's `[end_s, value]` pairs, values within 1e-9.
fn assert_points(got: &[SpeedPoint], expect: &Value, name: &str) {
    let want = points(expect);
    assert_eq!(got.len(), want.len(), "point count: {name}: got {got:?}");
    for (g, w) in got.iter().zip(&want) {
        assert_eq!(g.end_ms, w.end_ms, "end: {name}: got {got:?}");
        assert!((g.value - w.value).abs() < 1e-9, "value: {name}: got {got:?}");
    }
}

#[test]
fn speed_series_factors() {
    let fixture = fixture();
    let cases = fixture["factors"].as_array().unwrap();
    assert!(!cases.is_empty());
    for c in cases {
        let plan = c["plan"].as_str().unwrap();
        let want = c["factor"].as_f64().unwrap();
        assert!((max_factor(plan) - want).abs() < 1e-9, "factor: {plan}");
    }
}

#[test]
fn speed_series_cases() {
    let fixture = fixture();
    let cases = fixture["series"].as_array().unwrap();
    assert!(!cases.is_empty());
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let entries: Vec<Entry> = c["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| Entry { ms: ms(&p[0]), value: p[1].as_f64().unwrap() })
            .collect();
        let got = account_speed(
            &entries,
            c["step_min"].as_f64().unwrap() * 60_000.0,
            ms(&c["utc_offset_s"]),
            ms(&c["from"]),
            ms(&c["to"]),
            max_factor(c["plan"].as_str().unwrap()),
        );
        assert_points(&got, &c["expect"], name);
    }
}

#[test]
fn speed_series_total_cases() {
    let fixture = fixture();
    let cases = fixture["total"].as_array().unwrap();
    assert!(!cases.is_empty());
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let owned: Vec<(Vec<SpeedPoint>, f64)> = c["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (points(&a["points"]), ms(&a["last_entry"])))
            .collect();
        let accounts: Vec<AccountSpeed> = owned
            .iter()
            .map(|(p, last)| AccountSpeed { points: p, last_entry_ms: *last })
            .collect();
        let got = total_speed(&accounts, ms(&c["now"]));
        assert_points(&got, &c["expect"], name);
    }
}
