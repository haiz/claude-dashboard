//! Drives `contract/cases/animal-display.json`, the fixture the macOS
//! `AnimalDisplayContractTests` read too, against `core::animal_display`.

use claude_dashboard_core::animal_display::{
    asset_id, eligible_asset, AnimalActivityTracker, AnimalDisplayPhase, ContentMode,
};
use serde_json::Value;
use std::path::PathBuf;

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")) // .../apps/linux/core
        .parent().unwrap()  // apps/linux
        .parent().unwrap()  // apps
        .parent().unwrap()  // repo root
        .join("contract/cases/animal-display.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn cases<'a>(fixture: &'a Value, key: &str) -> &'a Vec<Value> {
    let list = fixture[key].as_array().unwrap_or_else(|| panic!("{key} missing"));
    assert!(!list.is_empty(), "{key} is empty");
    list
}

fn name(c: &Value) -> &str {
    c["name"].as_str().unwrap_or("<unnamed>")
}

fn content(raw: &str) -> ContentMode {
    match raw {
        "percentage" => ContentMode::Percentage,
        "percentageToAnimal" => ContentMode::PercentageToAnimal,
        "animal" => ContentMode::Animal,
        "animalToPercentage" => ContentMode::AnimalToPercentage,
        other => panic!("unknown content {other}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.0001
}

#[test]
fn phase_cases() {
    let f = fixture();
    for c in cases(&f, "phase_cases") {
        let n = name(c);
        let got = AnimalDisplayPhase::at(c["phase"].as_f64().unwrap());
        assert_eq!(got.content, content(c["expect_content"].as_str().unwrap()), "content: {n}");
        match (got.transition_progress, c["expect_transition_progress"].as_f64()) {
            (Some(a), Some(b)) => assert!(close(a, b), "progress {a} vs {b}: {n}"),
            (a, b) => assert_eq!(a, b, "progress: {n}"),
        }
        let pct = c["expect_percentage_opacity"].as_f64().unwrap();
        let animal = c["expect_animal_opacity"].as_f64().unwrap();
        assert!(close(got.percentage_opacity, pct), "percentage opacity: {n}");
        assert!(close(got.animal_opacity, animal), "animal opacity: {n}");
        let frame = c["expect_frame_index"].as_u64().map(|v| v as u32);
        assert_eq!(got.frame_index, frame, "frame: {n}");
    }
}

#[test]
fn asset_cases() {
    let f = fixture();
    for c in cases(&f, "asset_cases") {
        let emoji = c["emoji"].as_str().unwrap();
        assert_eq!(asset_id(emoji), c["expect_asset_id"].as_str(), "{}", name(c));
    }
}

#[test]
fn activity_cases() {
    let f = fixture();
    for c in cases(&f, "activity_cases") {
        let n = name(c);
        let mut tracker = AnimalActivityTracker::new();
        for op in c["operations"].as_array().unwrap() {
            let account = op["account_id"].as_str().unwrap();
            match op["kind"].as_str().unwrap() {
                "success" => tracker.record_success(
                    account,
                    op["utilization"].as_f64().unwrap(),
                    op["resets_at"].as_f64(),
                    op["at"].as_f64().unwrap(),
                ),
                // A failed refresh has no tracker operation by contract.
                "failed" => {}
                other => panic!("unknown operation {other}: {n}"),
            }
        }
        for check in c["checks"].as_array().unwrap() {
            let account = check["account_id"].as_str().unwrap();
            let at = check["at"].as_f64().unwrap();
            assert_eq!(
                tracker.is_active(account, at),
                check["expect_active"].as_bool().unwrap(),
                "{account} at {at}: {n}"
            );
        }
    }
}

#[test]
fn eligibility_cases() {
    let f = fixture();
    for c in cases(&f, "eligibility_cases") {
        let got = eligible_asset(
            c["show_burn_animals"].as_bool().unwrap(),
            c["is_five_hour"].as_bool().unwrap(),
            c["animal"].as_str(),
            c["is_active"].as_bool().unwrap(),
        );
        assert_eq!(got, c["expect_asset_id"].as_str(), "{}", name(c));
    }
}
