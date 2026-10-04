//! Rust -> Slint row bridge. `UiRow` is defined in `ui/app.slint`; the display
//! strings and colours are computed here from `core` so `.slint` never
//! re-derives them.

use claude_dashboard_core::burn_rate::BurnRateResult;
use claude_dashboard_core::colors::{avatar_color, countdown_color, usage_color, Rgb};
use claude_dashboard_core::format::{format_reset_time, formatted_countdown};
use claude_dashboard_core::model::{AccountPlan, AccountStatus};
use claude_dashboard_core::rows::{DisplayRow, WindowView};

use crate::{UiRow, UiWindow};

const FIVE_HOUR_S: f64 = 18000.0;
const SEVEN_DAY_S: f64 = 604800.0;

pub fn to_color(c: Rgb) -> slint::Color {
    let ch = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    slint::Color::from_rgb_u8(ch(c.r), ch(c.g), ch(c.b))
}

/// Local UTC offset in seconds (positive east). `format_reset_time` is
/// timezone-naive, so callers shift BOTH `now` and `reset` by this, in one
/// place (`window` below).
pub fn local_offset_s() -> f64 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    // Bias is minutes WEST of UTC; DaylightBias applies when id == 2.
    let bias = tz.Bias + if id == 2 { tz.DaylightBias } else { tz.StandardBias };
    -(bias as f64) * 60.0
}

fn plan_str(p: &AccountPlan) -> &'static str {
    match p {
        AccountPlan::Pro => "Pro",
        AccountPlan::Max5x => "Max 5x",
        AccountPlan::Max20x => "Max 20x",
        AccountPlan::Max200 => "Max",
    }
}

fn status_str(s: &AccountStatus) -> &'static str {
    match s {
        AccountStatus::Active => "Active",
        AccountStatus::Expired => "Expired",
        AccountStatus::Error => "Error",
    }
}

fn staleness(last: Option<f64>, now: f64) -> String {
    let Some(t) = last else { return "Never synced".into() };
    let d = (now - t).max(0.0) as i64;
    if d < 60 {
        "Updated just now".into()
    } else if d < 3600 {
        format!("Updated {}m ago", d / 60)
    } else if d < 86400 {
        format!("Updated {}h ago", d / 3600)
    } else {
        format!("Updated {}d ago", d / 86400)
    }
}

fn window(w: &Option<WindowView>, total_s: f64, now: f64, local_shift: f64) -> UiWindow {
    match w {
        None => UiWindow::default(),
        Some(v) => {
            let (reset_label, countdown, cd_color) = match v.resets_at_unix {
                Some(r) => {
                    let remaining = r - now;
                    (
                        format_reset_time(r + local_shift, total_s, now + local_shift),
                        formatted_countdown(remaining, total_s),
                        to_color(countdown_color(remaining, total_s)),
                    )
                }
                None => (String::new(), String::new(), slint::Color::from_rgb_u8(128, 128, 128)),
            };
            UiWindow {
                present: true,
                percent: v.utilization as f32,
                color: to_color(usage_color(v.utilization)),
                limited: v.is_limited,
                reset_label: reset_label.into(),
                countdown: countdown.into(),
                countdown_color: cd_color,
            }
        }
    }
}

pub fn to_ui_row(row: &DisplayRow, now_unix_s: f64) -> UiRow {
    let shift = local_offset_s();
    let seed = if row.account_id.is_empty() { &row.name } else { &row.account_id };
    let animal = row
        .burn_projected_seconds
        .map(|s| BurnRateResult::from_projected_time(s).animal)
        .unwrap_or("");
    UiRow {
        id: row.account_id.as_str().into(),
        name: row.name.as_str().into(),
        email: row.email.clone().unwrap_or_default().into(),
        initial: row
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            .into(),
        plan: plan_str(&row.plan).into(),
        status: status_str(&row.status).into(),
        peak: row.peak_utilization as f32,
        peak_color: to_color(usage_color(row.peak_utilization)),
        five_hour: window(&row.five_hour, FIVE_HOUR_S, now_unix_s, shift),
        seven_day: window(&row.seven_day, SEVEN_DAY_S, now_unix_s, shift),
        fable: window(&row.fable, SEVEN_DAY_S, now_unix_s, shift),
        avatar_color: to_color(avatar_color(seed)),
        animal: animal.into(),
        staleness: staleness(row.last_synced_unix, now_unix_s).into(),
        error: row.error.clone().unwrap_or_default().into(),
        extension_sourced: row.is_extension_sourced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> DisplayRow {
        DisplayRow {
            account_id: "a".into(),
            name: "work".into(),
            email: None,
            plan: AccountPlan::Pro,
            status: AccountStatus::Active,
            five_hour: Some(WindowView { utilization: 0.0, resets_at_unix: None, is_limited: false }),
            seven_day: None,
            fable: None,
            peak_utilization: 0.0,
            burn_projected_seconds: None,
            is_extension_sourced: false,
            error: None,
            last_synced_unix: None,
        }
    }

    #[test]
    fn error_maps_to_non_empty_string() {
        let mut r = row();
        r.error = Some("boom".into());
        assert_eq!(to_ui_row(&r, 1e9).error.as_str(), "boom");
        assert!(to_ui_row(&row(), 1e9).error.is_empty());
    }

    #[test]
    fn zero_usage_is_green_and_missing_window_absent() {
        let ui = to_ui_row(&row(), 1e9);
        assert!(ui.five_hour.present);
        assert!(!ui.seven_day.present);
        let c = ui.five_hour.color;
        assert!(c.green() > c.red() && c.green() > c.blue());
        assert_eq!(ui.plan.as_str(), "Pro");
        assert_eq!(ui.staleness.as_str(), "Never synced");
    }
}
