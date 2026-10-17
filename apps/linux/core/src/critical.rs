//! Whether an account's 7d quota will go unused. Pure port of macOS `CriticalPlanner`
//! (`docs/superpowers/specs/2026-10-16-critical-accounts-design.md`, "Assessment").
//!
//!     let a = assess(row.seven_day.as_ref(), &parameters_from(&settings, 16.3), now);
//!     a.level   // CriticalLevel::Critical

use chrono::{Duration, NaiveDate, TimeZone};
use chrono_tz::Tz;

use crate::rows::WindowView;
use crate::settings::Settings;

pub const WINDOW_HOURS: f64 = 5.0;
/// A week at or above this has nothing meaningful left; matches auto switch's 7d tier limit.
pub const SEVEN_DAY_LIMIT: f64 = 99.0;

/// Daily hours not counted as usable time, as whole hours in `tz`. A start after the end
/// crosses midnight (22 to 6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuietHours {
    pub start_hour: u32,
    pub end_hour: u32,
    pub tz: Tz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CriticalLevel {
    None,
    Critical,
    High,
}

impl CriticalLevel {
    pub fn label(self) -> &'static str {
        match self {
            CriticalLevel::None => "not critical",
            CriticalLevel::Critical => "critical",
            CriticalLevel::High => "high critical",
        }
    }
}

/// `waste`: the 7d percent still unused at the reset under the level's own pace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CriticalAssessment {
    pub level: CriticalLevel,
    pub waste: f64,
}

impl CriticalAssessment {
    pub const NONE: CriticalAssessment = CriticalAssessment { level: CriticalLevel::None, waste: 0.0 };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CriticalParameters {
    pub x: f64,
    pub pace: f64,
    pub high_pace: f64,
    pub quiet: Option<QuietHours>,
}

/// The settings' paces and quiet hours with the current X. Quiet hours are `None` when
/// start == end (macOS `CriticalSettings.quietHours`).
pub fn parameters_from(s: &Settings, x: f64) -> CriticalParameters {
    let quiet = (s.critical_quiet_start != s.critical_quiet_end).then(|| QuietHours {
        start_hour: s.critical_quiet_start.clamp(0, 23) as u32,
        end_hour: s.critical_quiet_end.clamp(0, 23) as u32,
        tz: s.critical_time_zone.parse().unwrap_or(chrono_tz::Asia::Ho_Chi_Minh),
    });
    CriticalParameters { x, pace: s.critical_pace as f64, high_pace: s.critical_high_pace as f64, quiet }
}

/// Every IANA zone name, sorted (the Settings time zone menu).
pub fn time_zone_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name()).collect();
    names.sort_unstable();
    names
}

/// `hour:00` on `day` in `tz`, as Unix seconds. An hour skipped by a DST jump moves forward
/// to the first valid time; a repeated one takes its first occurrence.
fn local_hour(tz: Tz, day: NaiveDate, hour: u32) -> f64 {
    let naive = day.and_hms_opt(hour, 0, 0).expect("hour in 0...23");
    let mut probe = naive;
    for _ in 0..4 {
        if let Some(t) = tz.from_local_datetime(&probe).earliest() {
            return t.timestamp() as f64;
        }
        probe += Duration::minutes(30);
    }
    chrono::Utc.from_utc_datetime(&naive).timestamp() as f64
}

/// Hours in `[start, end)` outside the quiet hours.
pub fn active_hours(start_unix: f64, end_unix: f64, quiet: Option<&QuietHours>) -> f64 {
    if end_unix <= start_unix {
        return 0.0;
    }
    let mut seconds = end_unix - start_unix;
    let Some(q) = quiet.filter(|q| q.start_hour != q.end_hour) else { return seconds / 3600.0 };
    let Some(start) = chrono::DateTime::from_timestamp(start_unix.floor() as i64, 0) else { return seconds / 3600.0 };
    // Start a day early: a block that began yesterday evening may still run.
    let mut day = start.with_timezone(&q.tz).date_naive() - Duration::days(1);
    while local_hour(q.tz, day, 0) < end_unix {
        let block_start = local_hour(q.tz, day, q.start_hour);
        let end_day = if q.end_hour > q.start_hour { day } else { day + Duration::days(1) };
        let block_end = local_hour(q.tz, end_day, q.end_hour);
        let overlap = block_end.min(end_unix) - block_start.max(start_unix);
        if overlap > 0.0 {
            seconds -= overlap;
        }
        day += Duration::days(1);
    }
    seconds.max(0.0) / 3600.0
}

/// 7d at the reset if each 5h window from now uses `pace`% of its limit.
pub fn projected(seven_day: f64, active_hours: f64, pace: f64, x: f64) -> f64 {
    seven_day + active_hours / WINDOW_HOURS * pace / 100.0 * x
}

pub fn assess(seven_day: Option<&WindowView>, params: &CriticalParameters, now_unix: f64) -> CriticalAssessment {
    let Some(w) = seven_day else { return CriticalAssessment::NONE };
    let Some(resets) = w.resets_at_unix else { return CriticalAssessment::NONE };
    if resets <= now_unix || params.x <= 0.0 || w.utilization >= SEVEN_DAY_LIMIT {
        return CriticalAssessment::NONE;
    }
    let seven = w.utilization;
    let high = projected(seven, active_hours(now_unix, resets, None), params.high_pace, params.x);
    if high <= 100.0 {
        return CriticalAssessment { level: CriticalLevel::High, waste: 100.0 - high };
    }
    let normal = projected(seven, active_hours(now_unix, resets, params.quiet.as_ref()), params.pace, params.x);
    if normal <= 100.0 {
        return CriticalAssessment { level: CriticalLevel::Critical, waste: 100.0 - normal };
    }
    CriticalAssessment::NONE
}

/// The flame's tooltip; `None` when not critical.
pub fn badge(a: &CriticalAssessment, params: &CriticalParameters) -> Option<String> {
    let reach = (100.0 - a.waste).round() as i64;
    let unused = a.waste.round() as i64;
    match a.level {
        CriticalLevel::None => None,
        CriticalLevel::High => Some(format!(
            "High critical: at {}% of 5h per window around the clock, 7d reaches {reach}% by its reset ({unused}% unused)",
            params.high_pace as i64
        )),
        CriticalLevel::Critical => {
            let quiet = params.quiet.map_or(" around the clock".to_string(), |q| {
                format!(" outside quiet hours ({:02}:00 to {:02}:00)", q.start_hour, q.end_hour)
            });
            Some(format!(
                "Critical: at {}% of 5h per window{quiet}, 7d reaches {reach}% by its reset ({unused}% unused)",
                params.pace as i64
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VN: Tz = chrono_tz::Asia::Ho_Chi_Minh;
    const NY: Tz = chrono_tz::America::New_York;

    /// A wall-clock time in `tz`, as Unix seconds.
    fn at(y: i32, m: u32, d: u32, h: u32, tz: Tz) -> f64 {
        tz.with_ymd_and_hms(y, m, d, h, 0, 0).single().unwrap().timestamp() as f64
    }

    fn quiet_vn() -> QuietHours {
        QuietHours { start_hour: 0, end_hour: 7, tz: VN }
    }

    fn params(quiet: Option<QuietHours>) -> CriticalParameters {
        CriticalParameters { x: 16.0, pace: 50.0, high_pace: 80.0, quiet }
    }

    fn week(seven: f64, resets: Option<f64>) -> WindowView {
        WindowView { utilization: seven, resets_at_unix: resets, is_limited: false }
    }

    const H: f64 = 3600.0;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn no_quiet_hours_counts_everything() {
        let now = at(2026, 10, 16, 10, VN);
        close(active_hours(now, now + 48.0 * H, None), 48.0);
    }

    #[test]
    fn two_days_lose_two_quiet_blocks() {
        let now = at(2026, 10, 16, 10, VN);
        close(active_hours(now, now + 48.0 * H, Some(&quiet_vn())), 34.0);
    }

    #[test]
    fn starting_inside_quiet_hours() {
        let now = at(2026, 10, 16, 3, VN);
        close(active_hours(now, at(2026, 10, 16, 12, VN), Some(&quiet_vn())), 5.0);
    }

    #[test]
    fn quiet_hours_crossing_midnight() {
        let q = QuietHours { start_hour: 22, end_hour: 6, tz: VN };
        close(active_hours(at(2026, 10, 16, 20, VN), at(2026, 10, 17, 8, VN), Some(&q)), 4.0);
    }

    #[test]
    fn quiet_hours_use_their_own_time_zone() {
        let q = QuietHours { start_hour: 0, end_hour: 7, tz: NY };
        close(active_hours(at(2026, 10, 16, 10, VN), at(2026, 10, 16, 20, VN), Some(&q)), 3.0);
    }

    #[test]
    fn daylight_saving_day_in_new_york() {
        let q = QuietHours { start_hour: 0, end_hour: 7, tz: NY };
        let (start, end) = (at(2026, 10, 31, 12, NY), at(2026, 11, 1, 12, NY));
        close((end - start) / H, 25.0);
        close(active_hours(start, end, Some(&q)), 17.0);
    }

    #[test]
    fn end_before_start_is_zero() {
        let now = at(2026, 10, 16, 10, VN);
        close(active_hours(now, now - 60.0, Some(&quiet_vn())), 0.0);
    }

    #[test]
    fn spec_example_is_critical() {
        let now = at(2026, 10, 16, 8, VN);
        let a = assess(Some(&week(55.0, Some(now + 25.0 * H))), &params(None), now);
        assert_eq!(a.level, CriticalLevel::Critical);
        close(a.waste, 5.0);
    }

    #[test]
    fn high_when_even_the_high_pace_around_the_clock_cannot_finish() {
        let now = at(2026, 10, 16, 8, VN);
        let a = assess(Some(&week(60.0, Some(now + 10.0 * H))), &params(Some(quiet_vn())), now);
        assert_eq!(a.level, CriticalLevel::High);
        close(a.waste, 14.4);
    }

    #[test]
    fn quiet_hours_can_make_an_account_critical() {
        let now = at(2026, 10, 16, 10, VN);
        let reset = Some(now + 48.0 * H);
        assert_eq!(assess(Some(&week(60.0, reset)), &params(None), now).level, CriticalLevel::None);
        assert_eq!(assess(Some(&week(60.0, reset)), &params(Some(quiet_vn())), now).level, CriticalLevel::None);
        assert_eq!(assess(Some(&week(50.0, reset)), &params(Some(quiet_vn())), now).level, CriticalLevel::None);
        assert_eq!(assess(Some(&week(40.0, reset)), &params(Some(quiet_vn())), now).level, CriticalLevel::Critical);
    }

    #[test]
    fn exactly_one_hundred_counts() {
        let now = at(2026, 10, 16, 8, VN);
        let a = assess(Some(&week(60.0, Some(now + 25.0 * H))), &params(None), now);
        assert_eq!(a.level, CriticalLevel::Critical);
        close(a.waste, 0.0);
    }

    #[test]
    fn week_not_started_missing_or_exhausted_is_not_critical() {
        let now = at(2026, 10, 16, 8, VN);
        assert_eq!(assess(Some(&week(0.0, None)), &params(None), now), CriticalAssessment::NONE);
        assert_eq!(assess(None, &params(None), now), CriticalAssessment::NONE);
        assert_eq!(assess(Some(&week(100.0, Some(now + H))), &params(None), now), CriticalAssessment::NONE);
        assert_eq!(assess(Some(&week(99.5, Some(now + H))), &params(None), now), CriticalAssessment::NONE);
        assert_eq!(assess(Some(&week(10.0, Some(now - H))), &params(None), now), CriticalAssessment::NONE, "reset passed");
        let zero_x = CriticalParameters { x: 0.0, ..params(None) };
        assert_eq!(assess(Some(&week(10.0, Some(now + H))), &zero_x, now), CriticalAssessment::NONE);
    }

    #[test]
    fn badge_text() {
        let p = params(Some(quiet_vn()));
        assert_eq!(badge(&CriticalAssessment::NONE, &p), None);
        assert_eq!(
            badge(&CriticalAssessment { level: CriticalLevel::High, waste: 12.4 }, &p).as_deref(),
            Some("High critical: at 80% of 5h per window around the clock, 7d reaches 88% by its reset (12% unused)")
        );
        assert_eq!(
            badge(&CriticalAssessment { level: CriticalLevel::Critical, waste: 5.0 }, &p).as_deref(),
            Some("Critical: at 50% of 5h per window outside quiet hours (00:00 to 07:00), 7d reaches 95% by its reset (5% unused)")
        );
        assert_eq!(
            badge(&CriticalAssessment { level: CriticalLevel::Critical, waste: 5.0 }, &params(None)).as_deref(),
            Some("Critical: at 50% of 5h per window around the clock, 7d reaches 95% by its reset (5% unused)")
        );
    }

    #[test]
    fn labels() {
        assert_eq!(CriticalLevel::None.label(), "not critical");
        assert_eq!(CriticalLevel::Critical.label(), "critical");
        assert_eq!(CriticalLevel::High.label(), "high critical");
    }

    #[test]
    fn parameters_follow_settings() {
        let mut s = Settings::default();
        let p = parameters_from(&s, 16.3);
        assert_eq!((p.x, p.pace, p.high_pace), (16.3, 50.0, 80.0));
        assert_eq!(p.quiet, Some(QuietHours { start_hour: 0, end_hour: 7, tz: VN }));
        s.critical_quiet_end = 0;
        assert_eq!(parameters_from(&s, 16.0).quiet, None, "start == end: no quiet hours");
        s.critical_time_zone = "Nowhere/Land".into();
        s.critical_quiet_end = 6;
        assert_eq!(parameters_from(&s, 16.0).quiet.unwrap().tz, VN, "unknown zone -> default");
    }

    #[test]
    fn zone_names_sorted_and_complete() {
        let names = time_zone_names();
        assert!(names.contains(&"Asia/Ho_Chi_Minh") && names.contains(&"Europe/Paris"));
        assert!(names.windows(2).all(|w| w[0] <= w[1]));
    }
}
