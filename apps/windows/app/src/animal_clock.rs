//! The one app-wide burn-animal clock (spec "Shared phase"). A single UI-thread `slint::Timer`
//! at 12 fps publishes core's `AnimalDisplayPhase` to the Slint `BurnAnimalClock` global of the
//! main window and the flyout; every 5h ring reads that global, so all cards share one phase,
//! a card that becomes eligible joins it, and a row refresh never restarts the animation.
//!
//! The timer runs only while it shows something: Show burn-rate animals on, a window visible,
//! and some row's 5h ring eligible (`animal-asset != ""` and the clock at or before
//! `animal-expires-at`). Each tick re-checks, so it stops after the last ring's 600 s lapse
//! without a refresh. Stopped, the global is [`ClockFrame::IDLE`]: percentages only.
//!
//! A tick writes the global only when what it draws changes ([`ClockFrame::drawn`]): during
//! the 5.5 s percent-only part of the cycle nothing is written, so nothing redraws. A minimized
//! main window keeps the (cheap) tick but skips the writes, so restoring it resumes at once.
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

    /// What the rings draw from this frame: while no animal frame is sampled every ring shows
    /// only its percent whatever the clock, so the clock is dropped and the frame is
    /// [`ClockFrame::IDLE`]. Two frames with equal `drawn()` render the same.
    pub fn drawn(self) -> Self {
        if self.sampled { self } else { ClockFrame::IDLE }
    }

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
/// `(has asset, expires at)` eligible (the contract keeps a ring eligible at exactly 600 s).
/// Pure.
pub fn needs_tick(show: bool, visible: bool, windows: impl IntoIterator<Item = (bool, f32)>, clock: f64) -> bool {
    show && visible && windows.into_iter().any(|(has_asset, expires)| has_asset && clock <= f64::from(expires))
}

/// The frame to write after `last` was written, or `None` when the rings would draw the same.
/// Pure.
pub fn to_publish(last: Option<ClockFrame>, next: ClockFrame) -> Option<ClockFrame> {
    let next = next.drawn();
    (last != Some(next)).then_some(next)
}

thread_local! {
    static TIMER: slint::Timer = slint::Timer::default();
    /// The frame last written to the globals; `None` forces the next write.
    static LAST: std::cell::Cell<Option<ClockFrame>> = const { std::cell::Cell::new(None) };
}

/// The main window counts as shown only while visible and not minimized; the flyout while visible.
fn shown(app: &AppWindow, pop: Option<&PopoverWindow>) -> bool {
    (app.window().is_visible() && !app.window().is_minimized()) || pop.is_some_and(|p| p.window().is_visible())
}

fn needed(app: &AppWindow, pop: Option<&PopoverWindow>, clock: f64) -> bool {
    // A minimized window still counts here: the tick keeps running (writing nothing, see
    // `tick`) because restoring a window triggers no `sync`.
    let visible = app.window().is_visible() || pop.is_some_and(|p| p.window().is_visible());
    let rows = app.get_account_rows();
    let windows = rows.iter().map(|r| (!r.five_hour.animal_asset.is_empty(), r.five_hour.animal_expires_at));
    needs_tick(app.get_show_burn_animals(), visible, windows, clock)
}

fn publish(app: &AppWindow, pop: Option<&PopoverWindow>, f: ClockFrame) {
    let Some(f) = to_publish(LAST.with(|l| l.get()), f) else { return };
    LAST.with(|l| l.set(Some(f)));
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
        if shown(app, pop) {
            publish(app, pop, ClockFrame::at(now));
        }
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
    // Rows or windows changed (a flyout may be new): write the current frame unconditionally.
    LAST.with(|l| l.set(None));
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
        assert!(needs_tick(true, true, live, 700.0), "still eligible at exactly 600 s (contract)");
        assert!(!needs_tick(true, true, live, 700.1), "expired after 600 s past the change");
        assert!(!needs_tick(false, true, live, 100.0), "setting off");
        assert!(!needs_tick(true, false, live, 100.0), "no window visible");
        assert!(!needs_tick(true, true, [(false, 900.0)], 100.0), "no asset");
        assert!(!needs_tick(true, true, [(true, -1.0)], 0.0), "no change since launch");
        assert!(!needs_tick(true, true, [], 0.0));
    }

    #[test]
    fn percent_only_ticks_write_nothing_and_sampled_ticks_write_each_change() {
        // First write after a sync always happens.
        let first = to_publish(None, ClockFrame::at(1.0)).expect("forced write");
        assert_eq!(first, ClockFrame::IDLE, "percent-only drops the clock");
        // The rest of the 5.5 s percent-only part writes nothing at 12 fps.
        let mut t = 1.0 + 1.0 / 12.0;
        while t < 5.5 {
            assert_eq!(to_publish(Some(first), ClockFrame::at(t)), None, "t={t}");
            t += 1.0 / 12.0;
        }
        // Sampled ticks change opacity or frame (and the clock the expiry compares) each time.
        let fade = to_publish(Some(first), ClockFrame::at(5.6)).expect("fade starts");
        assert!(fade.sampled && fade.clock > 5.5);
        let next = to_publish(Some(fade), ClockFrame::at(5.6 + 1.0 / 12.0)).expect("next fade step");
        assert!(next.animal_opacity > fade.animal_opacity);
        // Back to the percent at the cycle end: one write of IDLE, then quiet again.
        assert_eq!(to_publish(Some(next), ClockFrame::at(10.0)), Some(ClockFrame::IDLE));
        assert_eq!(to_publish(Some(ClockFrame::IDLE), ClockFrame::at(10.1)), None);
    }
}
