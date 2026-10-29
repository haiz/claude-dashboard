//! Rust -> Slint row bridge. `UiRow` is defined in `ui/app.slint`; the display
//! strings and colours are computed here from `core` so `.slint` never
//! re-derives them.

use claude_dashboard_core::burn_rate::BurnRateResult;
use claude_dashboard_core::colors::{avatar_color, countdown_color, usage_color, Rgb};
use claude_dashboard_core::geometry::{
    countdown_segments, fill_fraction, progress_arc, ring_center, ring_radius, segment_count_for,
    segment_range, to_angle, Metrics, METRICS, METRICS_REGULAR, TAU,
};
use claude_dashboard_core::format::{format_reset_time, formatted_countdown};
use claude_dashboard_core::model::{AccountPlan, AccountStatus};
use claude_dashboard_core::rows::{sidebar_percent, DisplayRow, WindowView};

use crate::animal_assets;
use claude_dashboard_core::animal_display::ACTIVITY_SECONDS;
use crate::{UiRow, UiSegment, UiWindow};

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

/// Gauge diameter / stroke for the card ring, from `core::geometry` metrics.
pub const GAUGE_DIAMETER: f64 = METRICS_REGULAR.large_diameter;
pub const GAUGE_LINE_WIDTH: f64 = METRICS_REGULAR.large_line_width;

fn pt(c: f64, r: f64, a: f64) -> (f64, f64) {
    (c + r * a.cos(), c + r * a.sin())
}

/// Slint `Path` commands for the utilization arc, built from
/// `core::geometry::progress_arc`. Empty at 0%; a full ring is two half arcs
/// (a single 360-degree arc is degenerate in SVG path syntax).
pub fn gauge_path(utilization: f64, diameter: f64, line_width: f64) -> String {
    if fill_fraction(utilization) <= 0.0 {
        return String::new();
    }
    let (start, end) = progress_arc(utilization);
    let sweep = end - start;
    let c = ring_center(diameter);
    let r = ring_radius(diameter, line_width);
    let (x0, y0) = pt(c, r, start);
    if sweep >= TAU - 1e-9 {
        let (x1, y1) = pt(c, r, start + TAU / 2.0);
        return format!(
            "M {x0:.3} {y0:.3} A {r:.3} {r:.3} 0 1 1 {x1:.3} {y1:.3} A {r:.3} {r:.3} 0 1 1 {x0:.3} {y0:.3}"
        );
    }
    let (x1, y1) = pt(c, r, end);
    let large = if sweep > TAU / 2.0 { 1 } else { 0 };
    format!("M {x0:.3} {y0:.3} A {r:.3} {r:.3} 0 {large} 1 {x1:.3} {y1:.3}")
}

/// Slint path for one arc between two ring fractions (0..1 of the full circle).
fn arc_path(from: f64, to: f64, diameter: f64, line_width: f64) -> String {
    let c = ring_center(diameter);
    let r = ring_radius(diameter, line_width);
    let (a0, a1) = (to_angle(from), to_angle(to));
    let (x0, y0) = pt(c, r, a0);
    let (x1, y1) = pt(c, r, a1);
    let large = if a1 - a0 > TAU / 2.0 { 1 } else { 0 };
    format!("M {x0:.3} {y0:.3} A {r:.3} {r:.3} 0 {large} 1 {x1:.3} {y1:.3}")
}

/// The countdown ring: one path per live segment, all in the countdown colour
/// (blue, turning green under 30% of the window left).
pub fn countdown_ring(remaining_s: f64, total_s: f64, diameter: f64, line_width: f64) -> Vec<UiSegment> {
    let color = to_color(countdown_color(remaining_s, total_s));
    countdown_segments(remaining_s, total_s, diameter, segment_count_for(total_s))
        .into_iter()
        .map(|(a, b)| UiSegment { path: arc_path(a, b, diameter, line_width).into(), color })
        .collect()
}

/// The countdown ring's grey track: every segment, with the same 2 px gaps as the fill
/// (macOS `CircularCountdownView` draws the track segmented too).
pub fn countdown_track(total_s: f64, diameter: f64, line_width: f64) -> Vec<slint::SharedString> {
    let n = segment_count_for(total_s);
    (0..n)
        .map(|i| {
            let (a, b) = segment_range(i, diameter, n);
            arc_path(a, b, diameter, line_width).into()
        })
        .collect()
}

/// Countdown fill and track for one ring size, or nothing when the window has no reset.
fn countdown_models(
    remaining_s: Option<f64>,
    total_s: f64,
    m: &Metrics,
) -> (slint::ModelRc<UiSegment>, slint::ModelRc<slint::SharedString>) {
    let (fill, track) = match remaining_s {
        Some(rem) => (
            countdown_ring(rem, total_s, m.small_diameter, m.small_line_width),
            countdown_track(total_s, m.small_diameter, m.small_line_width),
        ),
        None => (Vec::new(), Vec::new()),
    };
    (slint::ModelRc::new(slint::VecModel::from(fill)), slint::ModelRc::new(slint::VecModel::from(track)))
}

fn animal_for(r: &Option<BurnRateResult>) -> &'static str {
    r.as_ref().map(|r| r.animal).unwrap_or("")
}

/// The 5h, 7d and Fable animals of `row`: "" each when Settings > General > Show burn-rate
/// animals is off (`show_animals`), else the animal of that window's burn result ("" = unknown).
fn animals(row: &DisplayRow, show_animals: bool) -> [&'static str; 3] {
    if !show_animals {
        return ["", "", ""];
    }
    [
        animal_for(&row.burn),
        animal_for(&row.burn_7d),
        animal_for(&row.burn_fable),
    ]
}

fn window(
    w: &Option<WindowView>,
    total_s: f64,
    now: f64,
    local_shift: f64,
    animal: &str,
    countdown_ring_shown: bool,
) -> UiWindow {
    match w {
        None => UiWindow {
            diameter: GAUGE_DIAMETER as f32,
            line_width: GAUGE_LINE_WIDTH as f32,
            ..UiWindow::default()
        },
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
            let remaining = v.resets_at_unix.filter(|_| countdown_ring_shown).map(|r| r - now);
            let (segments, track) = countdown_models(remaining, total_s, &METRICS_REGULAR);
            let (segments_compact, track_compact) = countdown_models(remaining, total_s, &METRICS);
            UiWindow {
                present: true,
                percent: v.utilization as f32,
                color: to_color(usage_color(v.utilization)),
                limited: v.is_limited,
                fraction: fill_fraction(v.utilization) as f32,
                gauge_path: gauge_path(v.utilization, GAUGE_DIAMETER, GAUGE_LINE_WIDTH).into(),
                diameter: GAUGE_DIAMETER as f32,
                line_width: GAUGE_LINE_WIDTH as f32,
                reset_label: reset_label.into(),
                countdown: countdown.into(),
                countdown_color: cd_color,
                segments,
                segments_compact,
                track,
                track_compact,
                animal: animal.into(),
                animal_asset: Default::default(),
                animal_expires_at: NO_EXPIRY,
            }
        }
    }
}

/// `UiWindow.animal-expires-at` while the 5h value has not changed since launch.
const NO_EXPIRY: f32 = -1.0;

/// The animated asset id for the 5h ring: the stable id of `animal` while `show_animals` is on
/// and the packaged manifest lists it, else "" (percentage only). Activity is separate: see
/// [`set_animal_activity`].
fn five_hour_asset(animal: &str, show_animals: bool) -> slint::SharedString {
    if show_animals { animal_assets::asset_for_emoji(animal).unwrap_or("").into() } else { Default::default() }
}

/// Stamps the 5h window with when its animal activity lapses: `changed_at` (monotonic seconds,
/// `animal_assets::monotonic_now` scale) + 600 s, or -1 for "no change seen yet". The Slint
/// side shows the animal while `clock <= animal-expires-at`, so expiry needs no refresh.
/// Independent of the setting, so re-enabling resumes from the current timestamp.
pub fn set_animal_activity(ui: &mut UiRow, changed_at: Option<f64>) -> bool {
    let next = changed_at.map_or(NO_EXPIRY, |c| (c + ACTIVITY_SECONDS) as f32);
    let changed = ui.five_hour.animal_expires_at != next;
    ui.five_hour.animal_expires_at = next;
    changed
}

/// One window built from plain values, for the Help pane's sample gauge (macOS shows a
/// live `UsageBar` there).
pub fn sample_window(utilization: f64, resets_in_s: f64, total_s: f64, animal: &str, now_unix_s: f64) -> UiWindow {
    let view = WindowView { utilization, resets_at_unix: Some(now_unix_s + resets_in_s), is_limited: utilization >= 100.0 };
    window(&Some(view), total_s, now_unix_s, local_offset_s(), animal, true)
}

/// `show_animals` is Settings > General > Show burn-rate animals: off blanks every animal.
pub fn to_ui_row(row: &DisplayRow, now_unix_s: f64, show_animals: bool) -> UiRow {
    let shift = local_offset_s();
    let seed = if row.account_id.is_empty() { &row.name } else { &row.account_id };
    let [animal, animal_7d, animal_fable] = animals(row, show_animals);
    let sidebar = sidebar_percent(row);
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
        five_hour: UiWindow {
            animal_asset: five_hour_asset(animal, show_animals),
            ..window(&row.five_hour, FIVE_HOUR_S, now_unix_s, shift, animal, true)
        },
        seven_day: window(
            &row.seven_day,
            SEVEN_DAY_S,
            now_unix_s,
            shift,
            animal_7d,
            true,
        ),
        // macOS `showCountdown: false` for Fable: no countdown segments.
        fable: window(
            &row.fable,
            SEVEN_DAY_S,
            now_unix_s,
            shift,
            animal_fable,
            false,
        ),
        avatar_color: to_color(avatar_color(seed)),
        animal: animal.into(),
        staleness: staleness(row.last_synced_unix, now_unix_s).into(),
        error: row.error.clone().unwrap_or_default().into(),
        extension_sourced: row.is_extension_sourced,
        claude_code: row.is_active_claude_code,
        pinned: row.is_pinned,
        expired: row.status == AccountStatus::Expired,
        kind_label: row.kind_label.as_str().into(),
        expired_hint: row.expired_hint.as_str().into(),
        sidebar_percent: sidebar.unwrap_or(0.0) as f32,
        sidebar_color: to_color(usage_color(sidebar.unwrap_or(0.0))),
        has_sidebar_percent: sidebar.is_some(),
        loading: false,
        pinging: false,
        ping_caption: Default::default(),
        ping_failed: false,
        switch_state: Default::default(),
        switch_help: Default::default(),
        is_home: false,
        shows_home: false,
        critical_level: 0,
        critical_help: Default::default(),
    }
}

/// The 1 s tick (macOS `TimelineView(.periodic(by: 1))` on each countdown): rebuilds the
/// three windows of `ui` from `row` at `now` when their countdown text, reset label or
/// countdown colour changed, leaving every other field (error lines included) alone.
/// Returns whether anything changed, so callers only touch rows that need a redraw.
/// The animals are not timing: each window keeps the one it shows, and the 5h animal asset and
/// expiry (see [`set_animals`], [`set_animal_activity`]).
pub fn retime(ui: &mut UiRow, row: &DisplayRow, now: f64) -> bool {
    // The flag is irrelevant here: the fresh animals are dropped below.
    let fresh = to_ui_row(row, now, false);
    let mut changed = false;
    for (cur, new) in [
        (&mut ui.five_hour, fresh.five_hour),
        (&mut ui.seven_day, fresh.seven_day),
        (&mut ui.fable, fresh.fable),
    ] {
        if cur.countdown != new.countdown || cur.reset_label != new.reset_label || cur.countdown_color != new.countdown_color {
            let (animal, asset, expires) = (std::mem::take(&mut cur.animal), std::mem::take(&mut cur.animal_asset), cur.animal_expires_at);
            *cur = new;
            (cur.animal, cur.animal_asset, cur.animal_expires_at) = (animal, asset, expires);
            changed = true;
        }
    }
    changed
}

/// Show burn-rate animals was toggled: rewrites the row's and its windows' animals from
/// `row` for `show_animals`, leaving every other field alone. Returns whether anything changed.
pub fn set_animals(ui: &mut UiRow, row: &DisplayRow, now: f64, show_animals: bool) -> bool {
    let fresh = to_ui_row(row, now, show_animals);
    let mut changed = false;
    for (cur, new) in [
        (&mut ui.animal, fresh.animal),
        (&mut ui.five_hour.animal, fresh.five_hour.animal),
        (&mut ui.five_hour.animal_asset, fresh.five_hour.animal_asset),
        (&mut ui.seven_day.animal, fresh.seven_day.animal),
        (&mut ui.fable.animal, fresh.fable.animal),
    ] {
        if *cur != new {
            *cur = new;
            changed = true;
        }
    }
    changed
}

/// Tray tooltip text: the menu-bar row's 5h utilization and reset countdown.
pub fn tray_tooltip(row: Option<&DisplayRow>, now: f64) -> String {
    let Some(w) = row.and_then(|r| r.five_hour.as_ref()) else {
        return "Claude Dashboard - --".into();
    };
    // Truncated, like macOS `Int(utilization)`; the countdown is always `XhYYm`.
    let mut s = format!("Claude Dashboard - {}%", w.utilization as i64);
    if let Some(reset) = w.resets_at_unix {
        let mins = ((reset - now) / 60.0).floor() as i64;
        if mins > 0 {
            s.push_str(&format!(" \u{b7} resets in {}h{:02}m", mins / 60, mins % 60));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn tray_tooltip_with_reset() {
        let mut r = row();
        r.five_hour = Some(WindowView { utilization: 64.4, resets_at_unix: Some(1e9 + 72.0 * 60.0), is_limited: false });
        assert_eq!(tray_tooltip(Some(&r), 1e9), "Claude Dashboard - 64% \u{b7} resets in 1h12m");
        r.five_hour = Some(WindowView { utilization: 64.0, resets_at_unix: Some(1e9 + 45.0 * 60.0), is_limited: false });
        assert_eq!(tray_tooltip(Some(&r), 1e9), "Claude Dashboard - 64% \u{b7} resets in 0h45m");
        // Truncated, not rounded (macOS Int()).
        r.five_hour = Some(WindowView { utilization: 64.9, resets_at_unix: Some(1e9 + 125.0 * 60.0), is_limited: false });
        assert_eq!(tray_tooltip(Some(&r), 1e9), "Claude Dashboard - 64% \u{b7} resets in 2h05m");
    }

    #[test]
    fn tray_tooltip_no_data() {
        assert_eq!(tray_tooltip(None, 1e9), "Claude Dashboard - --");
    }

    #[test]
    fn tray_tooltip_no_reset_omits_suffix() {
        let mut r = row();
        r.five_hour = Some(WindowView { utilization: 12.0, resets_at_unix: None, is_limited: false });
        assert_eq!(tray_tooltip(Some(&r), 1e9), "Claude Dashboard - 12%");
        // A reset already in the past is omitted too.
        r.five_hour = Some(WindowView { utilization: 12.0, resets_at_unix: Some(1e9 - 5.0), is_limited: false });
        assert_eq!(tray_tooltip(Some(&r), 1e9), "Claude Dashboard - 12%");
    }

    #[test]
    fn claude_code_flag_maps_to_ui() {
        let mut r = row();
        assert!(!to_ui_row(&r, 1e9, true).claude_code);
        r.is_active_claude_code = true;
        assert!(to_ui_row(&r, 1e9, true).claude_code);
    }

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
            burn: None,
            burn_7d: None,
            burn_fable: None,
            is_extension_sourced: false,
            error: None,
            last_synced_unix: None,
            is_active_claude_code: false,
            is_pinned: false,
            kind_label: String::new(),
            expired_hint: String::new(),
        }
    }

    #[test]
    fn gauge_path_empty_at_zero_and_differs_by_usage() {
        let p0 = gauge_path(0.0, 68.0, 8.0);
        let p50 = gauge_path(50.0, 68.0, 8.0);
        let p100 = gauge_path(100.0, 68.0, 8.0);
        assert!(p0.is_empty());
        assert!(p50.starts_with("M "));
        assert_ne!(p50, p100);
        let mut r = row();
        r.five_hour = Some(WindowView { utilization: 50.0, resets_at_unix: None, is_limited: false });
        let ui = to_ui_row(&r, 1e9, true);
        assert!(!ui.five_hour.gauge_path.is_empty());
        assert!((ui.five_hour.fraction - 0.5).abs() < 1e-6);
    }

    #[test]
    fn countdown_segments_turn_green_under_30_percent() {
        let total = FIVE_HOUR_S;
        let blue = countdown_ring(total * 0.6, total, GAUGE_DIAMETER, GAUGE_LINE_WIDTH);
        let green = countdown_ring(total * 0.1, total, GAUGE_DIAMETER, GAUGE_LINE_WIDTH);
        assert!(!blue.is_empty() && !green.is_empty());
        let (b, g) = (blue[0].color, green[0].color);
        assert!(b.blue() > b.green(), "blue above 30% left");
        assert!(g.green() > g.blue() && g.green() > g.red(), "green under 30% left");
        assert!(blue.len() >= green.len());
        assert!(blue[0].path.starts_with("M "));
        assert!(countdown_ring(0.0, total, GAUGE_DIAMETER, GAUGE_LINE_WIDTH).is_empty());
    }

    #[test]
    fn sample_window_is_a_live_5h_gauge() {
        let w = sample_window(62.0, 8100.0, FIVE_HOUR_S, "\u{1F407}", 1e9);
        assert!(w.present);
        assert_eq!(w.countdown.as_str(), "2:15");
        assert!(w.segments.row_count() > 0 && w.track_compact.row_count() == 5);
        assert_eq!(w.animal.as_str(), "\u{1F407}");
    }

    #[test]
    fn retime_moves_the_countdown_and_keeps_other_fields() {
        let mut r = row();
        r.five_hour = Some(WindowView { utilization: 10.0, resets_at_unix: Some(1e9 + 600.0), is_limited: false });
        let mut ui = to_ui_row(&r, 1e9, true);
        ui.error = "kept".into();
        assert_eq!(ui.five_hour.countdown.as_str(), "10:00");
        assert!(!retime(&mut ui, &r, 1e9), "same second: nothing to redraw");
        assert!(retime(&mut ui, &r, 1e9 + 1.0));
        assert_eq!(ui.five_hour.countdown.as_str(), "9:59");
        assert_eq!(ui.error.as_str(), "kept");
    }

    #[test]
    fn countdown_track_has_one_path_per_segment_at_the_shown_size() {
        assert_eq!(countdown_track(FIVE_HOUR_S, 34.0, 4.0).len(), 5);
        assert_eq!(countdown_track(SEVEN_DAY_S, 44.0, 5.0).len(), 7);
        let mut r = row();
        r.five_hour = Some(WindowView { utilization: 10.0, resets_at_unix: Some(1e9 + 3600.0), is_limited: false });
        let ui = to_ui_row(&r, 1e9, true);
        assert_eq!(ui.five_hour.track.row_count(), 5);
        assert_eq!(ui.five_hour.track_compact.row_count(), 5);
        // Gaps are 2 px at the drawn diameter, so the two sizes differ.
        assert_ne!(ui.five_hour.track.row_data(0), ui.five_hour.track_compact.row_data(0));
        assert!(ui.five_hour.segments_compact.row_count() > 0);
    }

    #[test]
    fn every_window_carries_its_animal_and_fable_has_no_countdown() {
        let mut r = row();
        let reset = Some(1e9 + 3600.0);
        r.five_hour = Some(WindowView { utilization: 10.0, resets_at_unix: reset, is_limited: false });
        r.seven_day = Some(WindowView { utilization: 10.0, resets_at_unix: reset, is_limited: false });
        r.fable = Some(WindowView { utilization: 10.0, resets_at_unix: reset, is_limited: false });
        r.burn = Some(BurnRateResult::from_projected_time(600.0));
        r.burn_7d = Some(BurnRateResult::from_projected_time(1.0e6));
        r.burn_fable = Some(BurnRateResult::from_projected_time(600.0));
        let ui = to_ui_row(&r, 1e9, true);
        assert!(!ui.five_hour.animal.is_empty());
        assert!(!ui.seven_day.animal.is_empty());
        assert!(!ui.fable.animal.is_empty());
        assert_ne!(ui.five_hour.animal, ui.seven_day.animal);
        assert!(ui.five_hour.segments.row_count() > 0);
        assert!(ui.seven_day.segments.row_count() > 0);
        assert_eq!(ui.fable.segments.row_count(), 0);
    }

    fn burning_row() -> DisplayRow {
        let mut r = row();
        let reset = Some(1e9 + 3600.0);
        r.five_hour = Some(WindowView { utilization: 10.0, resets_at_unix: reset, is_limited: false });
        r.seven_day = Some(WindowView { utilization: 10.0, resets_at_unix: reset, is_limited: false });
        r.fable = Some(WindowView { utilization: 10.0, resets_at_unix: reset, is_limited: false });
        r.burn = Some(BurnRateResult::from_projected_time(600.0));
        r.burn_7d = Some(BurnRateResult::from_projected_time(1.0e6));
        r.burn_fable = Some(BurnRateResult::from_projected_time(600.0));
        r
    }

    #[test]
    fn rocket_burn_shows_the_rocket_and_hidden_animals_show_nothing() {
        let mut r = burning_row();
        r.burn = Some(BurnRateResult::from_rate(10.0, 0.0, 50.0));
        let on = to_ui_row(&r, 1e9, true);
        assert_eq!(on.five_hour.animal.as_str(), "\u{1F680}");
        assert_eq!(on.five_hour.animal_asset.as_str(), "rocket");
        let off = to_ui_row(&r, 1e9, false);
        assert!(off.five_hour.animal.is_empty() && off.five_hour.animal_asset.is_empty());
        // Fast refresh does not depend on the animals setting.
        assert_eq!(crate::refresh::burn_fast_ids(&[r], true), ["a"]);
    }

    #[test]
    fn show_animals_off_blanks_every_animal_and_on_keeps_them() {
        let r = burning_row();
        let off = to_ui_row(&r, 1e9, false);
        for a in [&off.animal, &off.five_hour.animal, &off.seven_day.animal, &off.fable.animal] {
            assert!(a.is_empty(), "off: no animal, got {a}");
        }
        let on = to_ui_row(&r, 1e9, true);
        assert_eq!(on.five_hour.animal.as_str(), "\u{1F406}");
        assert_eq!(on.seven_day.animal.as_str(), "\u{1F40C}");
        assert_eq!(on.fable.animal.as_str(), "\u{1F406}");
        assert_eq!(on.animal, on.five_hour.animal);
        // Display only: the gauges themselves are the same either way.
        assert_eq!(on.five_hour.percent, off.five_hour.percent);
        assert_eq!(on.five_hour.countdown, off.five_hour.countdown);
        assert_eq!(on.seven_day.gauge_path, off.seven_day.gauge_path);
    }

    #[test]
    fn set_animals_toggles_in_place_and_retime_keeps_them() {
        let r = burning_row();
        let mut ui = to_ui_row(&r, 1e9, false);
        ui.error = "kept".into();
        assert!(set_animals(&mut ui, &r, 1e9, true));
        assert!(!ui.seven_day.animal.is_empty());
        assert!(!set_animals(&mut ui, &r, 1e9, true), "unchanged: nothing to redraw");
        assert_eq!(ui.error.as_str(), "kept");
        // The 1 s tick never brings an animal back, nor drops one.
        assert!(set_animals(&mut ui, &r, 1e9, false));
        assert!(retime(&mut ui, &r, 1e9 + 1.0));
        assert!(ui.five_hour.animal.is_empty() && ui.seven_day.animal.is_empty());
        assert!(set_animals(&mut ui, &r, 1e9, true));
        assert!(retime(&mut ui, &r, 1e9 + 2.0));
        assert!(!ui.five_hour.animal.is_empty() && !ui.seven_day.animal.is_empty());
    }

    #[test]
    fn pin_expired_and_sidebar_fields_map_to_ui() {
        let mut r = row();
        r.is_pinned = true;
        r.status = AccountStatus::Expired;
        r.expired_hint = "hint".into();
        let ui = to_ui_row(&r, 1e9, true);
        assert!(ui.pinned && ui.expired && !ui.has_sidebar_percent);
        assert_eq!(ui.expired_hint.as_str(), "hint");
        r.status = AccountStatus::Active;
        r.five_hour = Some(WindowView { utilization: 42.0, resets_at_unix: None, is_limited: false });
        let ui = to_ui_row(&r, 1e9, true);
        assert!(ui.has_sidebar_percent);
        assert!((ui.sidebar_percent - 42.0).abs() < 1e-6);
    }

    #[test]
    fn error_maps_to_non_empty_string() {
        let mut r = row();
        r.error = Some("boom".into());
        assert_eq!(to_ui_row(&r, 1e9, true).error.as_str(), "boom");
        assert!(to_ui_row(&row(), 1e9, true).error.is_empty());
    }

    #[test]
    fn zero_usage_is_green_and_missing_window_absent() {
        let ui = to_ui_row(&row(), 1e9, true);
        assert!(ui.five_hour.present);
        assert!(!ui.seven_day.present);
        let c = ui.five_hour.color;
        assert!(c.green() > c.red() && c.green() > c.blue());
        assert_eq!(ui.plan.as_str(), "Pro");
        assert_eq!(ui.staleness.as_str(), "Never synced");
    }

    #[test]
    fn only_the_five_hour_window_carries_an_animal_asset() {
        let r = burning_row();
        let on = to_ui_row(&r, 1e9, true);
        let expected = animal_assets::asset_for_emoji("\u{1F406}").unwrap_or("");
        assert_eq!(on.five_hour.animal_asset.as_str(), expected);
        assert!(!expected.is_empty(), "600 s projection maps to a packaged animal");
        assert!(on.seven_day.animal_asset.is_empty() && on.fable.animal_asset.is_empty());
        // 7d / Fable animals are unchanged.
        assert_eq!(on.seven_day.animal.as_str(), "\u{1F40C}");
        assert_eq!(on.fable.animal.as_str(), "\u{1F406}");
    }

    #[test]
    fn setting_off_clears_the_asset_and_on_restores_it_with_the_same_expiry() {
        let r = burning_row();
        let mut ui = to_ui_row(&r, 1e9, true);
        assert!(set_animal_activity(&mut ui, Some(100.0)));
        assert_eq!(ui.five_hour.animal_expires_at, 700.0);
        assert!(set_animals(&mut ui, &r, 1e9, false));
        assert!(ui.five_hour.animal_asset.is_empty());
        assert_eq!(ui.five_hour.animal_expires_at, 700.0, "activity survives the setting");
        assert!(set_animals(&mut ui, &r, 1e9, true));
        assert!(!ui.five_hour.animal_asset.is_empty());
        assert_eq!(ui.five_hour.animal_expires_at, 700.0);
    }

    #[test]
    fn retime_keeps_asset_and_expiry_and_no_change_is_minus_one() {
        let r = burning_row();
        let mut ui = to_ui_row(&r, 1e9, true);
        assert_eq!(ui.five_hour.animal_expires_at, -1.0);
        set_animal_activity(&mut ui, Some(5.0));
        let asset = ui.five_hour.animal_asset.clone();
        assert!(retime(&mut ui, &r, 1e9 + 1.0));
        assert_eq!(ui.five_hour.animal_asset, asset);
        assert_eq!(ui.five_hour.animal_expires_at, 605.0);
        assert!(!set_animal_activity(&mut ui, Some(5.0)), "same stamp: nothing to redraw");
    }
}
