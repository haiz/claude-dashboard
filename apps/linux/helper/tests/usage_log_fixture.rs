//! Holds the Rust writer to the same byte-exact document the JS reference
//! implementation produces. Format: `contract/linux-usage-log.md`.

use claude_dashboard_helper::usage_log::UsageLog;
use serde::Deserialize;

#[derive(Deserialize)]
struct Recording {
    #[serde(rename = "accountId")]
    account_id: String,
    window: i64,
    #[serde(rename = "resetsAtMs")]
    resets_at_ms: i64,
    utilization: f64,
    #[serde(rename = "isLimited")]
    is_limited: bool,
    #[serde(rename = "recordedAtMs")]
    recorded_at_ms: i64,
}

#[derive(Deserialize)]
struct Fixture {
    recordings: Vec<Recording>,
    serialized: String,
}

fn fixture() -> Fixture {
    let repo = std::env::var("CLAUDE_DASHBOARD_REPO")
        .unwrap_or_else(|_| format!("{}/../..", env!("CARGO_MANIFEST_DIR")));
    let text = std::fs::read_to_string(format!("{repo}/contract/cases/linux-usage-log.json"))
        .expect("contract case linux-usage-log.json must be readable");
    serde_json::from_str(&text).expect("fixture must parse")
}

#[test]
fn the_rust_writer_reproduces_the_pinned_document() {
    let f = fixture();
    let mut log = UsageLog::new();
    for r in &f.recordings {
        log.record(&r.account_id, r.window, r.resets_at_ms, r.utilization, r.is_limited, r.recorded_at_ms);
    }
    assert_eq!(log.to_json(), f.serialized);
}

#[test]
fn a_round_trip_through_json_preserves_the_document() {
    let f = fixture();
    let reloaded = UsageLog::from_json(&f.serialized);
    assert_eq!(reloaded.to_json(), f.serialized);
}
