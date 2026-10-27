//! Burn-animal assets and activity for the Windows app. Pure (no Slint, no Windows API), so
//! the logic is unit-testable on any host.
//!
//! * Manifest: `ui/assets/burn-animals/manifest.json` is embedded at compile time and
//!   validated once ([`catalog`]). An invalid manifest, or an animal missing from it, makes
//!   [`asset_for_emoji`] return `None`: the percentage-only fallback, never a crash. The
//!   sheets themselves are referenced by compile-time `@image-url` literals in
//!   `ui/theme.slint` (`BurnAnimalSheets`), so a missing PNG fails the build.
//! * Time: activity expires 600 s after the last change even with no refresh, so it is stored
//!   as a timestamp on a monotonic clock ([`monotonic_now`], seconds since the first call)
//!   and re-checked by whoever draws ([`is_active`]). Callers pass the time in; tests never sleep.
//!
//!     let t = monotonic_now();
//!     record_activity(&mut tracker, &rows, t);          // after a successful fetch
//!     is_active(tracker.last_change_at("id"), t + 599.0) // true; false at +601

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use claude_dashboard_core::animal_display::{
    asset_id, AnimalActivityTracker, ACTIVITY_SECONDS, FRAME_COUNT, FRAME_RATE,
};
use claude_dashboard_core::model::AccountStatus;
use claude_dashboard_core::rows::DisplayRow;

const MANIFEST_JSON: &str = include_str!("../ui/assets/burn-animals/manifest.json");

/// The ids of the validated manifest's animals.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    ids: HashSet<String>,
}

impl Catalog {
    pub fn has(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
}

/// Checks `json` against the packaged contract: schema 1, 24 frames at 12 fps, a 6 x 4 grid of
/// 128 px cells (sheet 768 x 512), and every animal entry shaped `{id, sheet: "<id>.png"}`.
/// Frames are row-major by the numeric `columns`/`rows`; the free-text `frameOrder` is ignored.
pub fn validate_manifest(json: &str) -> Result<Catalog, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("malformed json: {e}"))?;
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).ok_or_else(|| format!("missing number {k}"));
    let expect = |k: &str, want: f64| {
        let got = num(k)?;
        if got == want { Ok(()) } else { Err(format!("{k} is {got}, expected {want}")) }
    };
    expect("schemaVersion", 1.0)?;
    expect("frameCount", FRAME_COUNT as f64)?;
    expect("framesPerSecond", FRAME_RATE)?;
    let (cols, rows, cw, ch) = (num("columns")?, num("rows")?, num("cellWidth")?, num("cellHeight")?);
    if cols * rows != FRAME_COUNT as f64 {
        return Err(format!("grid {cols}x{rows} does not hold {FRAME_COUNT} frames"));
    }
    expect("sheetWidth", cols * cw)?;
    expect("sheetHeight", rows * ch)?;
    let animals = v.get("animals").and_then(|a| a.as_array()).ok_or("missing animals")?;
    let mut ids = HashSet::new();
    for a in animals {
        let id = a.get("id").and_then(|x| x.as_str()).ok_or("animal without id")?;
        let sheet = a.get("sheet").and_then(|x| x.as_str()).ok_or("animal without sheet")?;
        if !["snail", "turtle", "rabbit", "horse", "cheetah"].contains(&id) {
            return Err(format!("unknown animal id {id}"));
        }
        if sheet != format!("{id}.png") {
            return Err(format!("{id} sheet is {sheet}"));
        }
        ids.insert(id.to_owned());
    }
    Ok(Catalog { ids })
}

/// The packaged manifest, validated once; `None` disables every animal (percentage only).
pub fn catalog() -> Option<&'static Catalog> {
    static CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();
    CATALOG
        .get_or_init(|| match validate_manifest(MANIFEST_JSON) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("burn animals disabled: {e}");
                None
            }
        })
        .as_ref()
}

/// Stable asset id for a burn-rate emoji, only when the manifest lists it.
pub fn asset_for_emoji(emoji: &str) -> Option<&'static str> {
    asset_id(emoji).filter(|id| catalog().is_some_and(|c| c.has(id)))
}

/// Seconds on a monotonic clock, zero at the first call (the app origin). Immune to wall-clock
/// jumps; the one clock activity timestamps and Task 6's phase tick share.
pub fn monotonic_now() -> f64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Recent activity: `changed_at` (monotonic seconds, `None` = never changed) is within 600 s of `now`.
pub fn is_active(changed_at: Option<f64>, now: f64) -> bool {
    changed_at.is_some_and(|c| now - c <= ACTIVITY_SECONDS)
}

/// Feeds the 5h window of every row this fetch measured successfully to `tracker`. Errored or
/// non-active rows (including ones backfilled with their last-good windows) are skipped, so a
/// failed refresh never extends activity. Same row filter as `refresh::apply_projections`.
pub fn record_activity(tracker: &mut AnimalActivityTracker, rows: &[DisplayRow], at: f64) {
    for row in rows {
        if row.error.is_some() || row.status != AccountStatus::Active {
            continue;
        }
        if let Some(w) = &row.five_hour {
            tracker.record_success(&row.account_id, w.utilization, w.resets_at_unix, at);
        }
    }
}

/// The app-wide tracker: written by the refresh thread, read by the UI thread.
pub fn shared_tracker() -> &'static Mutex<AnimalActivityTracker> {
    static T: OnceLock<Mutex<AnimalActivityTracker>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(AnimalActivityTracker::new()))
}

/// Monotonic time of the account's last 5h change in the shared tracker.
pub fn last_change(account_id: &str) -> Option<f64> {
    shared_tracker().lock().unwrap_or_else(|e| e.into_inner()).last_change_at(account_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::model::AccountPlan;
    use claude_dashboard_core::rows::WindowView;

    fn valid() -> String {
        MANIFEST_JSON.to_owned()
    }

    #[test]
    fn packaged_manifest_is_valid_and_lists_five_animals() {
        let c = validate_manifest(&valid()).expect("valid");
        for id in ["snail", "turtle", "rabbit", "horse", "cheetah"] {
            assert!(c.has(id));
        }
        assert_eq!(asset_for_emoji("🐇"), Some("rabbit"));
        assert_eq!(asset_for_emoji("🐈"), None);
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(validate_manifest("{not json").is_err());
    }

    #[test]
    fn wrong_dimensions_are_rejected() {
        let bad = valid().replace("\"sheetWidth\": 768", "\"sheetWidth\": 700");
        assert!(validate_manifest(&bad).unwrap_err().contains("sheetWidth"));
        let bad = valid().replace("\"frameCount\": 24", "\"frameCount\": 12");
        assert!(validate_manifest(&bad).is_err());
    }

    #[test]
    fn unknown_id_and_wrong_sheet_are_rejected() {
        let bad = valid().replace("\"id\": \"snail\"", "\"id\": \"sloth\"");
        assert!(validate_manifest(&bad).unwrap_err().contains("unknown animal id"));
        let bad = valid().replace("\"sheet\": \"snail.png\"", "\"sheet\": \"x.png\"");
        assert!(validate_manifest(&bad).is_err());
    }

    #[test]
    fn missing_entry_disables_only_that_animal() {
        let bad = valid().replace(
            "    {\n      \"id\": \"snail\",\n      \"sheet\": \"snail.png\"\n    },\n",
            "",
        );
        assert_ne!(bad, valid(), "fixture edit applied");
        let c = validate_manifest(&bad).expect("four animals still valid");
        assert!(!c.has("snail") && c.has("turtle"));
    }

    #[test]
    fn activity_expires_at_600_seconds_without_refresh() {
        assert!(!is_active(None, 5.0));
        assert!(is_active(Some(100.0), 100.0));
        assert!(is_active(Some(100.0), 700.0));
        assert!(!is_active(Some(100.0), 700.001));
    }

    fn row(id: &str, util: f64, status: AccountStatus, error: Option<&str>) -> DisplayRow {
        DisplayRow {
            account_id: id.into(),
            name: id.into(),
            email: None,
            plan: AccountPlan::Pro,
            status,
            five_hour: Some(WindowView { utilization: util, resets_at_unix: Some(50_000.0), is_limited: false }),
            seven_day: None,
            fable: None,
            peak_utilization: util,
            burn_projected_seconds: None,
            burn_projected_seconds_7d: None,
            burn_projected_seconds_fable: None,
            is_extension_sourced: false,
            error: error.map(str::to_owned),
            last_synced_unix: None,
            is_active_claude_code: false,
            is_pinned: false,
            kind_label: String::new(),
            expired_hint: String::new(),
        }
    }

    #[test]
    fn only_successful_active_rows_record_and_failures_never_extend() {
        let mut t = AnimalActivityTracker::new();
        record_activity(&mut t, &[row("a", 10.0, AccountStatus::Active, None)], 0.0);
        assert_eq!(t.last_change_at("a"), None, "first sample is a baseline");
        record_activity(&mut t, &[row("a", 20.0, AccountStatus::Active, None)], 100.0);
        assert_eq!(t.last_change_at("a"), Some(100.0));
        // Errored (backfilled, different utilization) and expired rows are ignored.
        record_activity(&mut t, &[row("a", 90.0, AccountStatus::Active, Some("boom"))], 200.0);
        record_activity(&mut t, &[row("a", 95.0, AccountStatus::Expired, None)], 300.0);
        assert_eq!(t.last_change_at("a"), Some(100.0));
        // Unchanged success does not extend.
        record_activity(&mut t, &[row("a", 20.0, AccountStatus::Active, None)], 400.0);
        assert_eq!(t.last_change_at("a"), Some(100.0));
        assert!(!is_active(t.last_change_at("a"), 701.0));
    }
}
