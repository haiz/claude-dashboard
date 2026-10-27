//! Pure display phase and recent-activity rules for burn-rate animals.

use std::collections::HashMap;

pub const CYCLE_SECONDS: f64 = 10.0;
pub const ACTIVITY_SECONDS: f64 = 600.0;
pub const RESET_TOLERANCE_SECONDS: f64 = 60.0;
pub const FRAME_COUNT: u32 = 24;
pub const FRAME_RATE: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    Percentage,
    PercentageToAnimal,
    Animal,
    AnimalToPercentage,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimalDisplayPhase {
    pub phase: f64,
    pub content: ContentMode,
    /// Progress from 0 to 1 during a cross-fade; `None` outside transitions.
    pub transition_progress: Option<f64>,
    pub percentage_opacity: f64,
    pub animal_opacity: f64,
    /// `None` while the animal is not sampled (phase 0 through 5.5).
    pub frame_index: Option<u32>,
}

impl AnimalDisplayPhase {
    pub fn at(elapsed_seconds: f64) -> Self {
        let phase = elapsed_seconds.rem_euclid(CYCLE_SECONDS);
        let (content, progress, percentage_opacity, animal_opacity, frame_index) =
            if phase < 5.5 {
                (ContentMode::Percentage, None, 1.0, 0.0, None)
            } else if phase < 6.0 {
                let p = (phase - 5.5) / 0.5;
                (ContentMode::PercentageToAnimal, Some(p), 1.0 - p, p, Some(0))
            } else if phase < 9.5 {
                let frame = (((phase - 6.0) * FRAME_RATE).floor() as u32) % FRAME_COUNT;
                (ContentMode::Animal, None, 0.0, 1.0, Some(frame))
            } else {
                let p = (phase - 9.5) / 0.5;
                let frame = (((phase - 6.0) * FRAME_RATE).floor() as u32) % FRAME_COUNT;
                (ContentMode::AnimalToPercentage, Some(p), p, 1.0 - p, Some(frame))
            };
        Self { phase, content, transition_progress: progress, percentage_opacity, animal_opacity, frame_index }
    }
}

/// Stable asset directory identifiers for the emoji emitted by BurnRateResult.
pub fn asset_id(emoji: &str) -> Option<&'static str> {
    match emoji {
        "🐌" => Some("snail"),
        "🐢" => Some("turtle"),
        "🐇" => Some("rabbit"),
        "🐎" => Some("horse"),
        "🐆" => Some("cheetah"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct Observation {
    utilization: f64,
    resets_at: Option<f64>,
}

#[derive(Debug, Default)]
pub struct AnimalActivityTracker {
    previous: HashMap<String, Observation>,
    last_change: HashMap<String, f64>,
}

impl AnimalActivityTracker {
    pub fn new() -> Self { Self::default() }

    /// Record a successful 5-hour observation. The first sample is baseline only.
    pub fn record_success(&mut self, account_id: &str, utilization: f64, resets_at: Option<f64>, at: f64) {
        let next = Observation { utilization, resets_at };
        if let Some(previous) = self.previous.get(account_id).copied() {
            let same_reset = match (previous.resets_at, resets_at) {
                (Some(a), Some(b)) => (a - b).abs() <= RESET_TOLERANCE_SECONDS,
                (a, b) => a == b,
            };
            if previous.utilization != utilization || !same_reset {
                self.last_change.insert(account_id.to_owned(), at);
            }
        }
        self.previous.insert(account_id.to_owned(), next);
    }

    /// Failed refreshes intentionally have no operation: they cannot extend activity.
    pub fn is_active(&self, account_id: &str, at: f64) -> bool {
        self.last_change.get(account_id).is_some_and(|changed| at - changed <= ACTIVITY_SECONDS)
    }

    pub fn last_change_at(&self, account_id: &str) -> Option<f64> {
        self.last_change.get(account_id).copied()
    }
}

/// Animated assets are eligible only for active 5-hour rows with the setting on.
pub fn eligible_asset(show_burn_animals: bool, is_five_hour: bool, emoji: Option<&str>, is_active: bool) -> Option<&'static str> {
    if !show_burn_animals || !is_five_hour || !is_active { return None; }
    emoji.and_then(asset_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_boundaries_opacity_and_frames() {
        let cases = [
            (0.0, ContentMode::Percentage, None, 1.0, 0.0, None),
            (5.5, ContentMode::PercentageToAnimal, Some(0.0), 1.0, 0.0, Some(0)),
            (6.0, ContentMode::Animal, None, 0.0, 1.0, Some(0)),
            (9.5, ContentMode::AnimalToPercentage, Some(0.0), 0.0, 1.0, Some(18)),
            (9.999, ContentMode::AnimalToPercentage, Some(0.998), 0.998, 0.002, Some(23)),
            (10.0, ContentMode::Percentage, None, 1.0, 0.0, None),
        ];
        for (t, content, progress, pct, animal, frame) in cases {
            let got = AnimalDisplayPhase::at(t);
            assert_eq!(got.content, content, "phase {t}");
            assert_eq!(got.frame_index, frame, "phase {t}");
            match (got.transition_progress, progress) {
                (Some(a), Some(b)) => assert!((a - b).abs() < 0.0001, "phase {t}: {a} vs {b}"),
                (a, b) => assert_eq!(a, b, "phase {t}"),
            }
            assert!((got.percentage_opacity - pct).abs() < 0.0001, "phase {t}");
            assert!((got.animal_opacity - animal).abs() < 0.0001, "phase {t}");
        }
    }

    #[test]
    fn stable_asset_mapping_and_eligibility() {
        for (emoji, id) in [("🐌", "snail"), ("🐢", "turtle"), ("🐇", "rabbit"), ("🐎", "horse"), ("🐆", "cheetah")] {
            assert_eq!(asset_id(emoji), Some(id));
        }
        assert_eq!(asset_id("🐈"), None);
        assert_eq!(eligible_asset(true, true, Some("🐇"), true), Some("rabbit"));
        assert_eq!(eligible_asset(false, true, Some("🐇"), true), None);
        assert_eq!(eligible_asset(true, false, Some("🐇"), true), None);
        assert_eq!(eligible_asset(true, true, None, true), None);
        assert_eq!(eligible_asset(true, true, Some("🐇"), false), None);
    }

    #[test]
    fn activity_changes_only_for_successful_changed_observations_and_is_account_scoped() {
        let mut tracker = AnimalActivityTracker::new();
        tracker.record_success("a", 10.0, Some(1000.0), 0.0);
        tracker.record_success("b", 10.0, Some(1000.0), 0.0);
        assert!(!tracker.is_active("a", 0.0));
        tracker.record_success("a", 11.0, Some(1000.0), 10.0);
        tracker.record_success("a", 11.0, Some(1060.0), 20.0);
        assert_eq!(tracker.last_change_at("a"), Some(10.0));
        assert!(tracker.is_active("a", 610.0));
        assert!(!tracker.is_active("a", 610.001));
        assert!(!tracker.is_active("b", 10.0));
        // A failed refresh is represented by no record_success call.
        assert!(!tracker.is_active("a", 610.001));
    }

    #[test]
    fn reset_cycle_change_activates_including_missing_boundary_difference() {
        let mut tracker = AnimalActivityTracker::new();
        tracker.record_success("a", 10.0, Some(1000.0), 0.0);
        tracker.record_success("a", 10.0, Some(1061.0), 10.0);
        assert_eq!(tracker.last_change_at("a"), Some(10.0));
    }
}
