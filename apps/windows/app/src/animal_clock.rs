//! The one app-wide burn-animal clock (spec "Shared phase"). A single UI-thread `slint::Timer`
//! at 12 fps publishes core's `AnimalDisplayPhase` to the Slint `BurnAnimalClock` global of the
//! main window and the flyout; every 5h ring reads that global, so all cards share one phase,
//! a card that becomes eligible joins it, and a row refresh never restarts the animation.
//!
//! The timer runs only while it shows something: Show burn-rate animals on, a window visible,
//! and some row's 5h ring eligible (`animal-asset != ""` and the clock before
//! `animal-expires-at`). Each tick re-checks, so it stops after the last ring's 600 s lapse
//! without a refresh. Stopped, the global is [`ClockFrame::IDLE`]: percentages only.
//!
//!     animal_clock::sync(&app, pop.as_ref()); // rows, the setting or a window's visibility changed

use std::time::Duration;

use claude_dashboard_core::animal_display::{AnimalDisplayPhase, FRAME_RATE};
use slint::{ComponentHandle, Model};

use crate::{animal_assets, AppWindow, BurnAnimalClock, PopoverWindow};

/// What the `BurnAnimalClock` global holds at one instant. Pure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockFrame {
    /// `animal_assets::monotonic_now()`, compared with each row's `animal-expires-at`.
    pub clock: f32,
    /// The phase samples an animal frame (5.5 s to 10 s of the 10 s cycle).
    pub sampled: bool,
    pub percent_opacity: f32,
    pub animal_opacity: f32,
    /// Top-left sheet pixel of the frame's cell (`animal_assets::cell_origin`).
    pub clip_x: i32,
    pub clip_y: i32,
}

impl ClockFrame {
    /// The stopped clock: percentage fully visible, no animal.
    pub const IDLE: ClockFrame = ClockFrame {
        clock: 0.0,
        sampled: false,
        percent_opacity: 1.0,
        animal_opacity: 0.0,
        clip_x: 0,
        clip_y: 0,
    };

    /// The frame at `clock` monotonic seconds; the phase is `clock` modulo 10 s (core).
    pub fn at(clock: f64) -> Self {
        let p = AnimalDisplayPhase::at(clock);
        let (sampled, (clip_x, clip_y)) = match p.frame_index {
            Some(i) => (true, animal_assets::cell_origin(i)),
            None => (false, (0, 0)),
        };
        ClockFrame {
            clock: clock as f32,
            sampled,
            percent_opacity: p.percentage_opacity as f32,
            animal_opacity: p.animal_opacity as f32,
            clip_x,
            clip_y,
        }
    }
}

/// Whether the tick must run at `clock`: the setting on, a window visible, and one 5h window
/// `(has asset, expires at)` eligible. Pure.
pub fn needs_tick(show: bool, visible: bool, windows: impl IntoIterator<Item = (bool, f32)>, clock: f64) -> bool {
    show && visible && windows.into_iter().any(|(has_asset, expires)| has_asset && clock < f64::from(expires))
}

thread_local! {
    static TIMER: slint::Timer = slint::Timer::default();
}

fn needed(app: &AppWindow, pop: Option<&PopoverWindow>, clock: f64) -> bool {
    let visible = app.window().is_visible() || pop.is_some_and(|p| p.window().is_visible());
    let rows = app.get_account_rows();
    let windows = rows.iter().map(|r| (!r.five_hour.animal_asset.is_empty(), r.five_hour.animal_expires_at));
    needs_tick(app.get_show_burn_animals(), visible, windows, clock)
}

fn publish(app: &AppWindow, pop: Option<&PopoverWindow>, f: ClockFrame) {
    let set = |g: BurnAnimalClock<'_>| {
        g.set_clock(f.clock);
        g.set_sampled(f.sampled);
        g.set_percent_opacity(f.percent_opacity);
        g.set_animal_opacity(f.animal_opacity);
        g.set_clip_x(f.clip_x);
        g.set_clip_y(f.clip_y);
    };
    set(app.global::<BurnAnimalClock>());
    if let Some(p) = pop {
        set(p.global::<BurnAnimalClock>());
    }
}

/// Publishes the frame at `now`, or stops the timer and shows percentages only. Returns
/// whether the clock keeps running.
fn tick(app: &AppWindow, pop: Option<&PopoverWindow>) -> bool {
    let now = animal_assets::monotonic_now();
    if needed(app, pop, now) {
        publish(app, pop, ClockFrame::at(now));
        true
    } else {
        TIMER.with(|t| t.stop());
        publish(app, pop, ClockFrame::IDLE);
        false
    }
}

/// Re-evaluates the clock after the rows, Show burn-rate animals or a window's visibility
/// changed: starts the one timer when a 5h ring needs it (never a second one), else stops it.
/// UI thread only.
pub fn sync(app: &AppWindow, pop: Option<&PopoverWindow>) {
    if !tick(app, pop) || TIMER.with(|t| t.running()) {
        return;
    }
    let (app_w, pop_w) = (app.as_weak(), pop.map(|p| p.as_weak()));
    let interval = Duration::from_secs_f64(1.0 / FRAME_RATE);
    TIMER.with(|t| {
        t.start(slint::TimerMode::Repeated, interval, move || {
            let Some(app) = app_w.upgrade() else {
                TIMER.with(|t| t.stop());
                return;
            };
            let pop = pop_w.as_ref().and_then(|w| w.upgrade());
            tick(&app, pop.as_ref());
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_follow_core_phase() {
        let pct = ClockFrame::at(3.0);
        assert_eq!(pct, ClockFrame { clock: 3.0, ..ClockFrame::IDLE });
        // 5.75 s: half-way cross-fade, entering animal holds frame 0.
        let fade_in = ClockFrame::at(5.75);
        assert!(fade_in.sampled && (fade_in.clip_x, fade_in.clip_y) == (0, 0));
        assert!((fade_in.percent_opacity - 0.5).abs() < 1e-6 && (fade_in.animal_opacity - 0.5).abs() < 1e-6);
        // 7.0 s: animal only, frame 12 = third row, first column.
        let animal = ClockFrame::at(7.0);
        assert_eq!((animal.percent_opacity, animal.animal_opacity), (0.0, 1.0));
        assert_eq!((animal.clip_x, animal.clip_y), (0, 256));
        // 9.75 s: fading out on frame 21 = fourth row, fourth column.
        let fade_out = ClockFrame::at(9.75);
        assert_eq!((fade_out.clip_x, fade_out.clip_y), (384, 384));
        assert!((fade_out.animal_opacity - 0.5).abs() < 1e-6);
        // The modulo boundary shows the percentage, no flash; later cycles repeat exactly.
        assert!(!ClockFrame::at(10.0).sampled);
        assert_eq!(ClockFrame::at(127.0).clip_y, animal.clip_y);
    }

    #[test]
    fn tick_runs_only_for_a_visible_eligible_ring_with_the_setting_on() {
        let live = [(false, 900.0), (true, 700.0)];
        assert!(needs_tick(true, true, live, 699.9));
        assert!(!needs_tick(true, true, live, 700.0), "expired at 600 s past the change");
        assert!(!needs_tick(false, true, live, 100.0), "setting off");
        assert!(!needs_tick(true, false, live, 100.0), "no window visible");
        assert!(!needs_tick(true, true, [(false, 900.0)], 100.0), "no asset");
        assert!(!needs_tick(true, true, [(true, -1.0)], 0.0), "no change since launch");
        assert!(!needs_tick(true, true, [], 0.0));
    }
}
