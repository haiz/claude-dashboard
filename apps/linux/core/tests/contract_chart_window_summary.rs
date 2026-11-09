//! Drives `contract/cases/chart-window-summary.json`, the fixture the macOS
//! `ChartWindowSummaryContractTests` read too, against `core::chart::window_summary`.

use claude_dashboard_core::chart::{window_summary, Entry, Range};
use serde_json::Value;
use std::path::PathBuf;

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")) // .../apps/linux/core
        .parent().unwrap()  // apps/linux
        .parent().unwrap()  // apps
        .parent().unwrap()  // repo root
        .join("contract/cases/chart-window-summary.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn ms(v: &Value) -> f64 {
    v.as_f64().unwrap() * 1000.0
}

#[test]
fn chart_window_summary_cases() {
    let fixture = fixture();
    let cases = fixture.as_array().unwrap();
    assert!(!cases.is_empty());
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let entries: Vec<Entry> = c["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| Entry { ms: ms(&p[0]), value: p[1].as_f64().unwrap() })
            .collect();
        let span = match c["window"].as_str().unwrap() {
            "5h" => Some(5.0 * 3_600_000.0),
            "7d" => Some(7.0 * 86_400_000.0),
            "F" => None,
            other => panic!("unknown window {other}"),
        };
        let range = Range { from_ms: ms(&c["from"]), to_ms: ms(&c["to"]) };
        let got = window_summary(&entries, range, ms(&c["now"]), span);
        let e = &c["expect"];
        assert_eq!(got.end_is_now, e["end_is_now"].as_bool().unwrap(), "end_is_now: {name}");
        assert_eq!(got.start_pct, e["start_pct"].as_i64(), "start_pct: {name}");
        assert_eq!(got.end_pct, e["end_pct"].as_i64(), "end_pct: {name}");
    }
}
