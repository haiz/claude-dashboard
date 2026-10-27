#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;

mod auto_switch;
mod ratio;
mod cc_login;
mod cc_flow;
mod cc_state;
mod chart;
mod chart_model;
mod help;
mod history;
mod overview;
mod instance;
mod migration;
mod model;
mod pin;
mod ping;
mod ping_log_view;
mod switch_log_view;
mod pipe;
mod popover;
mod refresh;
mod resync;
mod row_sync;
mod settings_accounts;
mod settings_general;
mod setup;
mod tips;
#[cfg(test)]
mod testenv;
mod tray;
mod updater;
mod wizard;

slint::include_modules!();

/// Apply Mica; returns false where unsupported (Windows 10) so the caller can
/// fall back to a solid background.
fn apply_mica(window: &slint::Window) -> bool {
    let binding = window.window_handle();
    let Ok(handle) = binding.window_handle() else {
        return false;
    };
    matches!(handle.as_raw(), RawWindowHandle::Win32(_))
        && window_vibrancy::apply_mica(handle, None).is_ok()
}

/// True when the system app theme is dark (AppsUseLightTheme == 0).
fn system_is_dark() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let val: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
    let mut data: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            val.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    rc == 0 && data == 0
}

thread_local! {
    /// The rows last published, so a dev-run pin toggle can re-publish them.
    static LAST_OUTPUT: std::cell::RefCell<Option<(refresh::RefreshOutput, f64)>> =
        const { std::cell::RefCell::new(None) };
    /// The flyout, reachable from `show_update_state` (banner) and the dev pin path.
    static POPOVER: std::cell::RefCell<Option<slint::Weak<PopoverWindow>>> =
        const { std::cell::RefCell::new(None) };
}

/// Per-account Re-sync All failure texts waiting for the refresh that follows, which
/// would otherwise replace the rows and drop them. Drained by `apply_output`.
static RESYNC_FAILURES: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    std::sync::LazyLock::new(Default::default);

/// The single fan-out for one refresh result: main window rows, popover rows
/// and the tray ring + tooltip (the macOS menu-bar row; the tray redraws only
/// when its value or tooltip changes). UI thread only.
fn apply_output(
    weak: &slint::Weak<AppWindow>,
    pop_weak: &slint::Weak<PopoverWindow>,
    out: &refresh::RefreshOutput,
    now: f64,
) {
    eprintln!("refresh: {} rows, peak {:.0}%", out.rows.len(), out.peak);
    let mut rows: Vec<UiRow> = out.rows.iter().map(|r| model::to_ui_row(r, now)).collect();
    set_ping_fields(&mut rows);
    set_switch_fields(&mut rows);
    set_auto_switch_fields(&mut rows, &out.rows, now, &claude_dashboard_core::settings::load());
    if let Some(pop) = pop_weak.upgrade() {
        set_footer(&pop, &out.rows);
    }
    // Re-sync All failures ride this refresh into the cards' error lines (macOS setError).
    let failures = std::mem::take(&mut *RESYNC_FAILURES.lock().unwrap_or_else(|e| e.into_inner()));
    for r in rows.iter_mut().filter(|r| !r.expired) {
        if let Some(msg) = failures.get(r.id.as_str()) {
            r.error = msg.as_str().into();
        }
    }
    if let Some(pop) = pop_weak.upgrade() {
        publish_rows(pop.get_account_rows(), rows.clone(), |m| pop.set_account_rows(m));
        pop.set_refreshing(false);
    }
    if let Some(app) = weak.upgrade() {
        use slint::Model;
        let pane_ids: Vec<slint::SharedString> = app.get_pane_rows().iter().map(|r| r.id).collect();
        let pane_rows = row_sync::stable_order(&pane_ids, rows.clone(), |r| r.id.clone());
        publish_rows(app.get_pane_rows(), pane_rows, |m| app.set_pane_rows(m));
        publish_rows(app.get_account_rows(), rows, |m| app.set_account_rows(m));
        app.set_refreshing(false);
        // The selected account is gone (removed here, in Settings, or by the store): fall back to Dashboard.
        if app.get_selection() == Pane::Account
            && !out.rows.iter().any(|r| r.account_id.as_str() == app.get_selected_account_id().as_str())
        {
            app.set_selection(Pane::Dashboard);
        }
        // Slide the open History chart to "now" and reload its data.
        history::on_refresh(&app);
        overview::on_refresh(&app);
    }
    let bar = claude_dashboard_core::rows::menu_bar_row(&out.rows);
    tray::update_label(
        bar.and_then(|r| r.five_hour.as_ref()).map(|w| w.utilization),
        &model::tray_tooltip(bar, now),
    );
    LAST_OUTPUT.with(|l| *l.borrow_mut() = Some((out.clone(), now)));
}

/// Copies each row's Ping row state (running, latest caption) from the ping controller.
fn set_ping_fields(rows: &mut [UiRow]) {
    let offset = model::local_offset_s();
    let c = ping::controller();
    for r in rows.iter_mut() {
        let (pinging, caption, failed) = c.ui_fields(r.id.as_str(), offset);
        r.pinging = pinging;
        r.ping_caption = caption.into();
        r.ping_failed = failed;
    }
}

/// Copies each row's Claude Code Switch state from the last capture.
fn set_switch_fields(rows: &mut [UiRow]) {
    for r in rows.iter_mut() {
        let (state, help) = cc_state::switch_fields(cc_state::availability(r.id.as_str()));
        r.switch_state = state.into();
        r.switch_help = help.into();
    }
}

/// Copies each row's bolt and flame (macOS `isPriorityTarget`, `showsPriorityControl`,
/// `criticalBadge`) from the settings, the Switch states and the current X.
fn set_auto_switch_fields(
    rows: &mut [UiRow],
    display: &[claude_dashboard_core::rows::DisplayRow],
    now: f64,
    settings: &claude_dashboard_core::settings::Settings,
) {
    let params = claude_dashboard_core::critical::parameters_from(settings, ratio::current().x);
    for r in rows.iter_mut() {
        let Some(d) = display.iter().find(|d| d.account_id == r.id.as_str()) else { continue };
        let f = auto_switch::card_fields(
            d,
            settings.auto_switch_priority_id.as_deref(),
            cc_state::availability(&d.account_id),
            settings.auto_switch_enabled,
            &params,
            now,
        );
        r.is_home = f.is_home;
        r.shows_home = f.shows_home;
        r.critical_level = f.critical_level;
        r.critical_help = f.critical_help.into();
    }
}

thread_local! {
    /// The X chart's points and model, for the hover readout.
    static RATIO_CHART: std::cell::RefCell<(Vec<claude_dashboard_core::usage_ratio::RatioPoint>, ratio::ChartModel)> =
        std::cell::RefCell::new(Default::default());
}

/// The Dashboard header's X button.
fn show_ratio_value(app: &AppWindow, v: &claude_dashboard_core::usage_ratio::RatioValue) {
    app.set_ratio_label(ratio::label(v).into());
    app.set_ratio_help(ratio::help(v, model::local_offset_s()).into());
    app.set_ratio_default(v.is_default());
}

fn tick_labels(ticks: &[(f64, String)]) -> slint::ModelRc<TickLabel> {
    let items: Vec<TickLabel> = ticks.iter().map(|(f, l)| TickLabel { frac: *f as f32, label: l.as_str().into() }).collect();
    slint::ModelRc::new(slint::VecModel::from(items))
}

/// UI thread: loads the X history for the picked range on a worker and draws it.
fn load_ratio_chart(weak: &slint::Weak<AppWindow>) {
    let Some(app) = weak.upgrade() else { return };
    let range = app.get_ratio_range().max(0) as usize;
    let w = weak.clone();
    std::thread::spawn(move || {
        let points = if dev_run() { Vec::new() } else { ratio::history(range, unix_now()) };
        let _ = slint::invoke_from_event_loop(move || {
            let Some(app) = w.upgrade() else { return };
            let m = ratio::chart(&points, model::local_offset_s());
            app.set_ratio_empty(points.is_empty());
            app.set_ratio_path(m.path.as_str().into());
            app.set_ratio_y_ticks(tick_labels(&m.y_ticks));
            app.set_ratio_x_ticks(tick_labels(&m.x_ticks));
            app.set_ratio_hovering(false);
            RATIO_CHART.with(|c| *c.borrow_mut() = (points, m));
        });
    });
}

/// The hover readout: the point nearest the pointer (macOS `UsageRatioChart.nearest`).
fn ratio_hover(app: &AppWindow, frac: f32) {
    RATIO_CHART.with(|c| {
        let (points, m) = &*c.borrow();
        let t = m.t_from + (frac as f64).clamp(0.0, 1.0) * (m.t_to - m.t_from);
        let Some(p) = ratio::nearest(points, t).map(|i| points[i]) else { return };
        app.set_ratio_hover_x(((p.t_unix - m.t_from) / (m.t_to - m.t_from)) as f32);
        app.set_ratio_hover_y((p.x / m.y_max) as f32);
        app.set_ratio_readout(ratio::readout(&p, model::local_offset_s()).into());
        app.set_ratio_hovering(true);
    });
}

/// The flyout footer's auto switch toggle and last event.
fn set_footer(pop: &PopoverWindow, rows: &[claude_dashboard_core::rows::DisplayRow]) {
    let event = auto_switch::controller().last_event.clone();
    let f = auto_switch::footer(&claude_dashboard_core::settings::load(), rows, event.as_ref(), model::local_offset_s());
    pop.set_auto_switch_shown(f.shown);
    pop.set_auto_switch_on(f.on);
    pop.set_auto_switch_near(f.near);
    pop.set_auto_switch_event(f.event.into());
    pop.set_auto_switch_tip(f.tip.into());
    pop.set_auto_switch_event_failed(f.failed);
}

/// Auto switch on or off, from the footer or Settings (macOS `setAutoSwitchEnabled`):
/// turning it on near a limit acts now, not at the next refresh.
fn set_auto_switch_enabled(on: bool) {
    let _ = claude_dashboard_core::settings::update(|s| s.auto_switch_enabled = on);
    refresh_auto_switch_ui();
    if let Some(tx) = CC_NUDGE.get() {
        let _ = tx.send(refresh::Nudge::Local);
    }
}

/// UI thread: redraws every card's bolt and flame in place (a tick, a settings change, a new X).
fn refresh_auto_switch_ui() {
    use slint::Model;
    if let Some(a) = MAIN_APP.with(|m| m.borrow().as_ref().and_then(|w| w.upgrade())) {
        sync_switch_settings_ui(&a);
    }
    let Some((out, now)) = LAST_OUTPUT.with(|l| l.borrow().clone()) else { return };
    let settings = claude_dashboard_core::settings::load();
    let patch = |rows: slint::ModelRc<UiRow>| {
        for i in 0..rows.row_count() {
            let Some(mut r) = rows.row_data(i) else { continue };
            let before = (r.is_home, r.shows_home, r.critical_level, r.critical_help.clone());
            set_auto_switch_fields(std::slice::from_mut(&mut r), &out.rows, now, &settings);
            if (r.is_home, r.shows_home, r.critical_level, r.critical_help.clone()) != before {
                rows.set_row_data(i, r);
            }
        }
    };
    if let Some(a) = MAIN_APP.with(|m| m.borrow().as_ref().and_then(|w| w.upgrade())) {
        patch(a.get_account_rows());
        patch(a.get_pane_rows());
    }
    if let Some(p) = POPOVER.with(|p| p.borrow().as_ref().and_then(|w| w.upgrade())) {
        patch(p.get_account_rows());
        set_footer(&p, &out.rows);
    }
}

/// Settings > General's Auto Switch and Critical Accounts rows, from settings.json.
fn sync_switch_settings_ui(app: &AppWindow) {
    let s = settings_general::switch_state(&claude_dashboard_core::settings::load());
    app.set_switch_settings(SwitchSettings {
        on: s.on,
        five: s.five as i32,
        seven: s.seven as i32,
        widget: s.widget,
        pace: s.pace as i32,
        high_pace: s.high_pace as i32,
        quiet_start: s.quiet_start as i32,
        quiet_end: s.quiet_end as i32,
        zone: s.zone.into(),
    });
}

/// The bolt (macOS `togglePriority`): ticks `id` (moving the tick off any other account) or
/// unticks it; a tick runs auto switch now, on the current rows.
fn toggle_home(id: &str) {
    let Some((out, _)) = LAST_OUTPUT.with(|l| l.borrow().clone()) else { return };
    let Some(row) = out.rows.iter().find(|r| r.account_id == id) else { return };
    let current = auto_switch::Config::from_settings(&claude_dashboard_core::settings::load()).priority;
    let next = auto_switch::toggled_priority(current.as_ref(), row, unix_now());
    let ticked = next.is_some();
    let _ = claude_dashboard_core::settings::update(|s| {
        s.auto_switch_priority_id = next.as_ref().map(|p| p.account_id.clone());
        s.auto_switch_priority_baseline = next.and_then(|p| p.baseline);
    });
    refresh_auto_switch_ui();
    if ticked {
        if let Some(tx) = CC_NUDGE.get() {
            let _ = tx.send(refresh::Nudge::Local);
        }
    }
}

/// UI thread: redraws the Ping rows after a ping started or finished.
fn refresh_ping_ui(weak: &slint::Weak<AppWindow>) {
    use slint::Model;
    let Some(app) = weak.upgrade() else { return };
    for rows in [app.get_account_rows(), app.get_pane_rows()] {
        for i in 0..rows.row_count() {
            let Some(mut r) = rows.row_data(i) else { continue };
            let before = (r.pinging, r.ping_caption.clone(), r.ping_failed);
            set_ping_fields(std::slice::from_mut(&mut r));
            if (r.pinging, r.ping_caption.clone(), r.ping_failed) != before {
                rows.set_row_data(i, r);
            }
        }
    }
}

/// Updates the `VecModel` behind `current` in place by account id (see `row_sync`), so the
/// repeaters keep their components; `set` installs a fresh model the first time.
fn publish_rows(current: slint::ModelRc<UiRow>, rows: Vec<UiRow>, set: impl FnOnce(slint::ModelRc<UiRow>)) {
    use slint::Model;
    match current.as_any().downcast_ref::<slint::VecModel<UiRow>>() {
        Some(vm) => row_sync::sync(vm, rows, |r| r.id.clone()),
        None => set(slint::ModelRc::new(slint::VecModel::from(rows))),
    }
}


/// Background loop: refresh now, then every Auto Refresh interval (re-read each
/// cycle, so a settings change applies without restart) or whenever nudged. A
/// `Nudge::One` cycle fetches only that account and keeps the other rows (and their
/// burn projections) from the previous output.
fn spawn_refresh_loop(
    rx: std::sync::mpsc::Receiver<refresh::Nudge>,
    tx: std::sync::mpsc::Sender<refresh::Nudge>,
    weak: slint::Weak<AppWindow>,
    pop_weak: slint::Weak<PopoverWindow>,
) {
    use refresh::Nudge;
    std::thread::spawn(move || {
        let mut prev: Vec<claude_dashboard_core::rows::DisplayRow> = Vec::new();
        let mut tracker = claude_dashboard_core::burn_rate::BurnRateTracker::default();
        let mut timer = refresh::AutoTimer::default();
        let mut poll = refresh::FastPoll::default();
        let mut poll_target: Option<String> = None;
        let mut jobs = vec![Nudge::All];
        loop {
            // `coalesce` drops `Reschedule`: an empty batch just waits again below,
            // with the interval re-read from settings.
            for job in std::mem::take(&mut jobs) {
                if job == Nudge::Reschedule {
                    continue;
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                if job != Nudge::Local {
                    // The header Refresh buttons stay disabled until apply_output.
                    let (w, pw) = (weak.clone(), pop_weak.clone());
                    let _ = slint::invoke_from_event_loop(move || begin_refresh(&w, &pw));
                }
                let mut out = match &job {
                    Nudge::Local => match claude_dashboard_core::store::load_accounts() {
                        Ok(accounts) => refresh::relabel(&prev, &accounts, now),
                        Err(e) => refresh::merge_errors(&prev, Err(e.to_string()), now),
                    },
                    Nudge::All | Nudge::Reschedule => {
                        let fresh = refresh::refresh_once(now);
                        let fetched = fresh.is_ok();
                        let mut out = refresh::merge_errors(&prev, fresh, now);
                        // Only a real fetch is a new measurement; a failed refresh keeps the previous projections.
                        if fetched {
                            refresh::apply_projections(&mut tracker, &mut out.rows, now);
                        }
                        out
                    }
                    Nudge::One(_) | Nudge::Some(_) => {
                        let ids = match &job {
                            Nudge::One(id) => vec![id.clone()],
                            Nudge::Some(ids) => ids.clone(),
                            _ => Vec::new(),
                        };
                        let mut out = refresh::refresh_some(&ids, &prev, now);
                        if out.rows != prev {
                            for i in 0..out.rows.len() {
                                if ids.contains(&out.rows[i].account_id) {
                                    refresh::apply_projections(&mut tracker, &mut out.rows[i..=i], now);
                                }
                            }
                        }
                        out
                    }
                };
                out.peak = claude_dashboard_core::rows::peak_utilization(&out.rows);
                // Claude Code: copy the active login into the vault, recompute Switch states
                // (macOS `refreshClaudeCodeSwitchState`, part of every refresh).
                if cc_state::switcher().is_some() {
                    if let Ok(accounts) = claude_dashboard_core::store::load_accounts() {
                        cc_state::refresh(&accounts);
                    }
                }
                // Auto Switch after every refresh (macOS `refreshAll` -> `runAutoSwitch`), then the
                // 60 s poll of the active account while it is near a limit.
                poll_target = run_auto_switch(&mut out, now);
                poll.update(poll_target.is_some(), std::time::Instant::now());
                if job == Nudge::All {
                    poll.rearm(std::time::Instant::now());
                }
                // Auto Ping on this refresh's own fetches (macOS `refreshAll`); never in dev runs.
                let enabled = !dev_run() && claude_dashboard_core::settings::load().auto_ping_enabled;
                let targets = ping::controller().claim_targets(&out.candidates, enabled, now);
                spawn_auto_pings(targets, tx.clone());
                prev = out.rows.clone();
                let (weak, pop_weak) = (weak.clone(), pop_weak.clone());
                let _ = slint::invoke_from_event_loop(move || apply_output(&weak, &pop_weak, &out, now));
            }
            // The Auto Refresh tick keeps its own schedule: nudged refreshes never push it
            // back; a `Reschedule` (settings change) or the tick itself restarts it.
            let interval = refresh::loop_timeout(&claude_dashboard_core::settings::load());
            let tick = timer.wait(interval, std::time::Instant::now());
            let poll_wait = poll.wait(std::time::Instant::now());
            let wait = match (tick, poll_wait) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let batch = match wait {
                Some(t) => match rx.recv_timeout(t) {
                    Ok(n) => pending(n, &rx),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        let now = std::time::Instant::now();
                        if timer.wait(interval, now).is_some_and(|left| left.is_zero()) {
                            timer.restart(interval, now);
                            vec![Nudge::All]
                        } else if poll.fire(now) {
                            // The poll fetches only Claude Code's account (macOS `refreshAll(only:)`).
                            poll_target.clone().map(Nudge::One).into_iter().collect()
                        } else {
                            Vec::new()
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                },
                // Auto refresh off: wait for a nudge only.
                None => match rx.recv() {
                    Ok(n) => pending(n, &rx),
                    Err(_) => return,
                },
            };
            if batch.contains(&Nudge::Reschedule) {
                let interval = refresh::loop_timeout(&claude_dashboard_core::settings::load());
                timer.restart(interval, std::time::Instant::now());
            }
            jobs = refresh::coalesce(batch);
        }
    });
}

pub(crate) fn unix_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Refresh thread: auto switch on this refresh's rows (macOS `runAutoSwitch`). A switch
/// re-captures Claude Code's login and moves the badge in `out`. Returns the account the 60 s
/// poll should fetch, while auto switch is on and it is near a limit. Never in dev runs (no
/// switcher), and skipped while a manual Switch runs.
fn run_auto_switch(out: &mut refresh::RefreshOutput, now: f64) -> Option<String> {
    use claude_dashboard_core::cc_switch::switcher::{SwitchAccount, SwitchError};
    let switcher = cc_state::switcher()?;
    let settings = claude_dashboard_core::settings::load();
    let mut cfg = auto_switch::Config::from_settings(&settings);
    let params = claude_dashboard_core::critical::parameters_from(&settings, ratio::current().x);
    let mut accounts = auto_switch::accounts_from(&out.rows, cc_state::availability, &params, now);
    if let Some(active) = accounts.iter_mut().find(|a| a.is_active) {
        if let Ok(log) = claude_dashboard_core::store::UsageLogStore::open() {
            active.five_hour_log = auto_switch::log_points(&log.series(&active.id, 0, now - 3600.0, now + 1.0));
        }
    }
    // An unreadable store is no evidence of anything: skip this evaluation.
    let stored = claude_dashboard_core::store::load_accounts().ok();
    let guard = stored.as_ref().and_then(|_| cc_state::try_begin_switch());
    if let (Some(stored), Some(guard)) = (stored, guard) {
        let ids: Vec<String> = stored.iter().map(|a| a.id.clone()).collect();
        auto_switch::drop_deleted_home(&mut cfg, &ids);
        let log_store = claude_dashboard_core::auto_switch_log::AutoSwitchLogStore::open().ok();
        let before = auto_switch::Config::from_settings(&settings).priority;
        // Evaluate on a copy: a switch can block for seconds, and the UI reads the footer event.
        let mut c = auto_switch::controller().clone();
        let switched = c.evaluate(
            &mut cfg,
            &accounts,
            now,
            |n| (uuid::Uuid::new_v4().as_u128() % n.max(1) as u128) as usize,
            &mut |e| {
                if let Some(s) = &log_store {
                    s.insert(&e);
                }
            },
            |id| {
                let target = stored.iter().find(|a| a.id == id).ok_or(SwitchError::NotCaptured)?;
                let target = SwitchAccount { id: target.id.clone(), email: target.email.clone() };
                // The Switch buttons are disabled meanwhile (macOS `isSwitchingClaudeCode`).
                let _ = slint::invoke_from_event_loop(|| cc_set_switching(true));
                let result = switcher.switch_to(&target, &cc_state::switch_accounts(&stored)).map(|_| ());
                let _ = slint::invoke_from_event_loop(|| cc_set_switching(false));
                result
            },
        );
        *auto_switch::controller() = c;
        drop(guard);
        if cfg.priority != before {
            let after = cfg.priority.clone();
            let _ = claude_dashboard_core::settings::update(|s| auto_switch::write_back_priority(s, &before, &after));
        }
        if switched.is_some() {
            cc_state::refresh(&stored);
            let candidates = std::mem::take(&mut out.candidates);
            *out = refresh::relabel(&out.rows, &stored, now);
            out.candidates = candidates;
            accounts = auto_switch::accounts_from(&out.rows, cc_state::availability, &params, now);
        }
    }
    let active = accounts.iter().find(|a| a.is_active)?.id.clone();
    auto_switch::controller().should_fast_poll(&cfg, &accounts).then_some(active)
}

/// Pings the claimed accounts in parallel on a worker, then refreshes them in one pass
/// (macOS `runAutoPings`). A claim whose ping never ran (one was already running) is released.
fn spawn_auto_pings(targets: Vec<ping::PingTarget>, tx: std::sync::mpsc::Sender<refresh::Nudge>) {
    if targets.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        std::thread::scope(|s| {
            for t in &targets {
                s.spawn(move || {
                    if ping::run_ping(t, claude_dashboard_core::ping_log::PingTrigger::Auto).is_none() {
                        ping::controller().release_claim(&t.account_id);
                    }
                });
            }
        });
        let _ = tx.send(refresh::Nudge::Some(targets.into_iter().map(|t| t.account_id).collect()));
    });
}

/// UI thread: reads the Ping Log on a worker and shows it, names from the current rows.
fn load_ping_log(weak: &slint::Weak<AppWindow>) {
    use slint::Model;
    let Some(app) = weak.upgrade() else { return };
    let names: std::collections::HashMap<String, String> =
        app.get_account_rows().iter().map(|r| (r.id.to_string(), r.name.to_string())).collect();
    let w = weak.clone();
    std::thread::spawn(move || {
        let entries = if dev_run() {
            Vec::new()
        } else {
            claude_dashboard_core::ping_log::PingLogStore::open()
                .map(|log| log.fetch(claude_dashboard_core::ping_log::MAX_ENTRIES))
                .unwrap_or_default()
        };
        let offset = model::local_offset_s();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(a) = w.upgrade() {
                let rows = ping_log_view::rows(&entries, &names, offset);
                a.set_ping_log_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
            }
        });
    });
}

/// UI thread: reads the Switch Log on a worker and shows it. Dev runs show none.
fn load_switch_log(weak: &slint::Weak<AppWindow>) {
    let w = weak.clone();
    std::thread::spawn(move || {
        let entries = if dev_run() {
            Vec::new()
        } else {
            claude_dashboard_core::auto_switch_log::AutoSwitchLogStore::open()
                .map(|log| log.fetch(claude_dashboard_core::auto_switch_log::MAX_ENTRIES))
                .unwrap_or_default()
        };
        let offset = model::local_offset_s();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(a) = w.upgrade() {
                let rows = switch_log_view::rows(&entries, offset);
                a.set_switch_log_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
            }
        });
    });
}

// ---- Claude Code Switch flow (macOS `requestSwitchClaudeCode` / `switchClaudeCode`) ----

/// UI thread only: the confirmation waiting for the user, the running switch, the browser
/// wait it can cancel, and which prompt is up (`CcPrompt` kind).
#[derive(Default)]
struct CcFlow {
    pending: Option<String>,
    switching: bool,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    kind: i32,
}

thread_local! {
    static CC_FLOW: std::cell::RefCell<CcFlow> = std::cell::RefCell::new(CcFlow::default());
    static MAIN_APP: std::cell::RefCell<Option<slint::Weak<AppWindow>>> = const { std::cell::RefCell::new(None) };
}

/// Refreshes after a switch (`Nudge::Local`: the badge moves and the rows re-sort at once).
static CC_NUDGE: std::sync::OnceLock<std::sync::mpsc::Sender<refresh::Nudge>> = std::sync::OnceLock::new();

/// Applies `f` to the main window and `g` to the flyout, whichever exist.
fn cc_ui(f: impl Fn(&AppWindow), g: impl Fn(&PopoverWindow)) {
    if let Some(a) = MAIN_APP.with(|m| m.borrow().as_ref().and_then(|w| w.upgrade())) {
        f(&a);
    }
    if let Some(p) = POPOVER.with(|p| p.borrow().as_ref().and_then(|w| w.upgrade())) {
        g(&p);
    }
}

/// Shows prompt `kind` (0 hides it) in both windows.
fn cc_prompt(kind: i32, title: &str, text: &str) {
    CC_FLOW.with(|f| f.borrow_mut().kind = kind);
    if kind != 0 {
        let app = MAIN_APP.with(|m| m.borrow().as_ref().and_then(|w| w.upgrade()));
        let flyout_visible = POPOVER.with(|p| p.borrow().as_ref().and_then(|w| w.upgrade())).is_some_and(|p| p.window().is_visible());
        if let Some(a) = app.filter(|a| cc_flow::needs_main_window(flyout_visible, a.window().is_visible())) {
            show_main(&a);
            raise_to_foreground(a.window());
        }
    }
    let (t, x) = (slint::SharedString::from(title), slint::SharedString::from(text));
    cc_ui(
        |a| {
            a.set_cc_prompt_title(t.clone());
            a.set_cc_prompt_text(x.clone());
            a.set_cc_prompt_kind(kind);
        },
        |p| {
            p.set_cc_prompt_title(t.clone());
            p.set_cc_prompt_text(x.clone());
            p.set_cc_prompt_kind(kind);
        },
    );
}

fn cc_set_switching(on: bool) {
    CC_FLOW.with(|f| f.borrow_mut().switching = on);
    cc_ui(|a| a.set_cc_switching(on), |p| p.set_cc_switching(on));
}

fn row_name(r: &claude_dashboard_core::rows::DisplayRow) -> String {
    r.email.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| r.name.clone())
}

/// Every Switch button: while the account Claude Code uses now still has quota left, ask
/// first; otherwise switch straight away.
fn cc_request(id: String) {
    let rows = LAST_OUTPUT.with(|l| l.borrow().as_ref().map(|(o, _)| o.rows.clone())).unwrap_or_default();
    let active = rows.iter().find(|r| r.is_active_claude_code);
    if let (Some(active), Some(target)) = (active, rows.iter().find(|r| r.account_id == id)) {
        let usage = active.five_hour.as_ref().zip(active.seven_day.as_ref()).map(|(f, s)| (f.utilization, s.utilization));
        if active.account_id != id && cc_flow::needs_confirm(usage) {
            let (five, seven) = usage.unwrap_or_default();
            let text = cc_flow::confirm_message(&row_name(active), five, seven, &row_name(target));
            CC_FLOW.with(|f| f.borrow_mut().pending = Some(id));
            cc_prompt(1, cc_flow::CONFIRM_TITLE, &text);
            return;
        }
    }
    cc_start(id);
}

fn cc_start(id: String) {
    use claude_dashboard_core::cc_switch::switcher::Availability;
    if cc_state::switcher().is_none() || CC_FLOW.with(|f| f.borrow().switching) {
        return;
    }
    // The Switch Log row: names and usage as they were before the switch.
    let rows = LAST_OUTPUT.with(|l| l.borrow().as_ref().map(|(o, _)| o.rows.clone())).unwrap_or_default();
    if cc_state::availability(&id) == Some(Availability::NeedsLogin) {
        let name = rows.iter().find(|r| r.account_id == id).map(row_name).unwrap_or_default();
        let message = cc_flow::needs_login_message(&name);
        log_manual_switch(&rows, &id, Some(message.clone()));
        cc_prompt(2, cc_flow::MESSAGE_TITLE, &message);
        return;
    }
    // An auto switch may be running on the refresh thread (macOS disables Switch meanwhile).
    let Some(guard) = cc_state::try_begin_switch() else { return };
    let usage = rows.iter().find(|r| r.account_id == id).and_then(auto_switch::usage_of);
    cc_set_switching(true);
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    CC_FLOW.with(|f| f.borrow_mut().cancel = Some(cancel.clone()));
    std::thread::spawn(move || {
        let message = cc_run(&id, &cancel, usage, |error| log_manual_switch(&rows, &id, error));
        drop(guard);
        let _ = slint::invoke_from_event_loop(move || cc_finish(message));
    });
}

/// A Switch button press into the Switch Log; `error` is the message the user saw.
fn log_manual_switch(rows: &[claude_dashboard_core::rows::DisplayRow], id: &str, error: Option<String>) {
    if let Ok(store) = claude_dashboard_core::auto_switch_log::AutoSwitchLogStore::open() {
        store.insert(&auto_switch::manual_entry(rows, id, error, unix_now()));
    }
}

/// Worker: signs the account in first when its vault copy is missing or expired, then
/// switches. The message to show, or `None` (the user cancelled the browser wait). `log`
/// gets the Switch Log outcome: `None` for a switch, the message for a failure; a cancel
/// is not logged.
fn cc_run(
    id: &str,
    cancel: &std::sync::atomic::AtomicBool,
    usage: Option<claude_dashboard_core::auto_switch::Usage>,
    log: impl Fn(Option<String>),
) -> Option<String> {
    use claude_dashboard_core::cc_switch::switcher::{Availability, SwitchAccount};
    let switcher = cc_state::switcher()?;
    let accounts = claude_dashboard_core::store::load_accounts().unwrap_or_default();
    let account = accounts.iter().find(|a| a.id == id)?;
    let name = account.email.clone().unwrap_or_else(|| account.name.clone());
    if matches!(cc_state::availability(id), Some(Availability::NotCaptured | Availability::LoginExpired)) {
        let sources = claude_dashboard_core::extension_sources::load(&claude_dashboard_core::extension_sources::sources_path())
            .unwrap_or_default();
        let Some(input) = cc_flow::provision_input(account, &sources) else {
            let message = cc_flow::resync_first_message(&name);
            log(Some(message.clone()));
            return Some(message);
        };
        let on_awaiting = || {
            let _ = slint::invoke_from_event_loop(|| cc_prompt(3, cc_flow::WAITING_TITLE, cc_flow::WAITING_TEXT));
        };
        match cc_login::provision(&input, cancel, on_awaiting, &cc_login::RealDeps, cc_login::BROWSER_TIMEOUT) {
            Ok(entry) => {
                if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    return None;
                }
                if switcher.store(id, &entry).is_err() {
                    let message = cc_flow::store_failed_message(&name);
                    log(Some(message.clone()));
                    return Some(message);
                }
            }
            Err(e) => {
                let message = e.message(&name);
                if message.is_some() {
                    log(message.clone());
                }
                return message;
            }
        }
    }
    let target = SwitchAccount { id: account.id.clone(), email: account.email.clone() };
    let message = match switcher.switch_to(&target, &cc_state::switch_accounts(&accounts)) {
        Ok(_) => {
            // Hold Claude Code here until its 5h reset, so the critical rules do not undo it.
            auto_switch::controller().note_manual_switch(id, usage, unix_now());
            log(None);
            cc_flow::success_message(&name)
        }
        Err(e) => {
            let message = e.message(&name);
            log(Some(message.clone()));
            message
        }
    };
    cc_state::refresh(&accounts);
    if let Some(tx) = CC_NUDGE.get() {
        let _ = tx.send(refresh::Nudge::Local);
    }
    Some(message)
}

fn cc_finish(message: Option<String>) {
    cc_set_switching(false);
    CC_FLOW.with(|f| f.borrow_mut().cancel = None);
    match message {
        Some(m) => cc_prompt(2, cc_flow::MESSAGE_TITLE, &m),
        None => cc_prompt(0, "", ""),
    }
}

/// The prompt's main button: Switch on the confirmation, OK on a message.
fn cc_ok() {
    match CC_FLOW.with(|f| f.borrow().kind) {
        1 => {
            let pending = CC_FLOW.with(|f| f.borrow_mut().pending.take());
            cc_prompt(0, "", "");
            if let Some(id) = pending {
                cc_start(id);
            }
        }
        2 => cc_prompt(0, "", ""),
        _ => {}
    }
}

/// Cancel: drops a pending confirmation, or stops waiting for the browser.
fn cc_cancel() {
    match CC_FLOW.with(|f| f.borrow().kind) {
        1 => {
            CC_FLOW.with(|f| f.borrow_mut().pending = None);
            cc_prompt(0, "", "");
        }
        3 => {
            if let Some(c) = CC_FLOW.with(|f| f.borrow().cancel.clone()) {
                c.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            cc_prompt(0, "", "");
        }
        _ => {}
    }
}

/// What a manual ping needs for account `id`: an active stored account with an org and a
/// session key (macOS `pingTarget`).
fn ping_target(id: &str) -> Option<ping::PingTarget> {
    use claude_dashboard_core::{model::AccountStatus, store};
    let a = store::load_accounts().ok()?.into_iter().find(|a| a.id == id)?;
    if a.status != AccountStatus::Active {
        return None;
    }
    let session_key = a.session_key.as_deref().and_then(store::decrypt_session_key)?;
    Some(ping::PingTarget { account_id: a.id, org_id: a.org_id?, session_key })
}

/// The first nudge plus whatever else is already queued (not yet folded: the loop reads
/// `Reschedule` out of it before `coalesce` drops it).
fn pending(first: refresh::Nudge, rx: &std::sync::mpsc::Receiver<refresh::Nudge>) -> Vec<refresh::Nudge> {
    let mut all = vec![first];
    all.extend(rx.try_iter());
    all
}


/// True for dev runs (`FAKE_ROWS`, `SMOKE`) that must never touch the real store.
fn dev_run() -> bool {
    std::env::var_os("CLAUDE_DASHBOARD_FAKE_ROWS").is_some()
        || std::env::var_os("CLAUDE_DASHBOARD_SMOKE").is_some()
}

/// UI thread: sets `f` on the row `id` of the main window's models (cards and pages).
fn patch_row(weak: &slint::Weak<AppWindow>, id: &str, f: impl Fn(&mut UiRow)) {
    use slint::Model;
    let Some(app) = weak.upgrade() else { return };
    for rows in [app.get_account_rows(), app.get_pane_rows()] {
        if let Some(i) = (0..rows.row_count()).find(|&i| rows.row_data(i).is_some_and(|r| r.id == id)) {
            if let Some(mut row) = rows.row_data(i) {
                f(&mut row);
                rows.set_row_data(i, row);
            }
        }
    }
}

/// Every second: moves the countdowns on every card and page (macOS redraws them each
/// second) and the tray tooltip. Every 5 s: once an active window's reset passed more than
/// 10 s ago, refreshes everything, once per reset (macOS `checkResetTriggers`).
fn start_clock(
    weak: slint::Weak<AppWindow>,
    pop_weak: slint::Weak<PopoverWindow>,
    tx: std::sync::mpsc::Sender<refresh::Nudge>,
) -> slint::Timer {
    use slint::Model;
    let fired = std::cell::RefCell::new(std::collections::HashSet::new());
    let ticks = std::cell::Cell::new(0u32);
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(1), move || {
        let Some(rows) = LAST_OUTPUT.with(|l| l.borrow().as_ref().map(|(o, _)| o.rows.clone())) else { return };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let retime_all = |m: slint::ModelRc<UiRow>| {
            for i in 0..m.row_count() {
                let Some(mut ui) = m.row_data(i) else { continue };
                let Some(r) = rows.iter().find(|r| r.account_id == ui.id.as_str()) else { continue };
                if model::retime(&mut ui, r, now) {
                    m.set_row_data(i, ui);
                }
            }
        };
        if let Some(app) = weak.upgrade() {
            retime_all(app.get_account_rows());
            retime_all(app.get_pane_rows());
        }
        if let Some(pop) = pop_weak.upgrade() {
            retime_all(pop.get_account_rows());
        }
        let bar = claude_dashboard_core::rows::menu_bar_row(&rows);
        tray::update_label(bar.and_then(|r| r.five_hour.as_ref()).map(|w| w.utilization), &model::tray_tooltip(bar, now));

        ticks.set(ticks.get() + 1);
        if ticks.get().is_multiple_of(5) && !dev_run() {
            let mut fired = fired.borrow_mut();
            let new = refresh::passed_resets(&rows, now).into_iter().filter(|k| fired.insert(k.clone())).count();
            if new > 0 {
                let _ = tx.send(refresh::Nudge::All);
            }
        }
    });
    timer
}

/// UI thread: raises `refreshing` (disables the Refresh buttons) until the next
/// `apply_output`. Cards get no spinner: macOS never sets `isLoading` during a refresh.
fn begin_refresh(weak: &slint::Weak<AppWindow>, pop_weak: &slint::Weak<PopoverWindow>) {
    if let Some(app) = weak.upgrade() {
        app.set_refreshing(true);
    }
    if let Some(pop) = pop_weak.upgrade() {
        pop.set_refreshing(true);
    }
}

/// Card pin toggle and per-card Re-sync. Store I/O runs on a worker; a successful
/// pin or re-sync nudges a refresh, a failed re-sync shows its text in that card's
/// error line until the next refresh replaces the rows.
fn install_card_actions(app: &AppWindow, nudge_tx: std::sync::mpsc::Sender<refresh::Nudge>) {
    let (w, tx) = (app.as_weak(), nudge_tx.clone());
    app.on_toggle_pin(move |id, pinned| {
        let id = id.to_string();
        if dev_run() {
            // Fake rows: flip the flags locally and re-publish (the tray follows the pin);
            // the real store is left alone.
            let last = LAST_OUTPUT.with(|l| l.borrow_mut().take());
            if let Some((mut out, now)) = last {
                for r in out.rows.iter_mut() {
                    r.is_pinned = if r.account_id == id { pinned } else { r.is_pinned && !pinned };
                }
                let pop = POPOVER.with(|p| p.borrow().clone()).unwrap_or_default();
                apply_output(&w, &pop, &out, now);
            }
            return;
        }
        let (tx, w) = (tx.clone(), w.clone());
        std::thread::spawn(move || match pin::set_pinned(&id, pinned) {
            // Re-sort from the store; no fetch (macOS `togglePin`).
            Ok(()) => {
                let _ = tx.send(refresh::Nudge::Local);
            }
            Err(e) => {
                eprintln!("pin failed: {e}");
                let msg = format!("Could not change the pin. {e}");
                let _ = slint::invoke_from_event_loop(move || {
                    patch_row(&w, &id, |r| r.error = msg.as_str().into());
                });
            }
        });
    });
    // Settings > Accounts "Re-sync All": scanned accounts are re-read on a worker with
    // "Re-syncing… (n/total)" progress, then one Nudge::All refreshes everything.
    let (w, tx) = (app.as_weak(), nudge_tx.clone());
    app.on_resync_all(move || {
        use slint::Model;
        let Some(app) = w.upgrade() else { return };
        if app.get_resyncing() {
            return;
        }
        let ids: Vec<String> = app.get_account_rows().iter().map(|r| r.id.to_string()).collect();
        if ids.is_empty() {
            return;
        }
        // Dev runs: walk the progress over the fake rows without touching the store.
        let dev_total = dev_run().then_some(ids.len());
        app.set_resyncing(true);
        // Until the worker counts the scanned accounts.
        app.set_resync_progress(settings_accounts::progress_text(0, ids.len()).into());
        let (w, tx) = (w.clone(), tx.clone());
        std::thread::spawn(move || {
            let post = |text: Option<String>| {
                let w = w.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(a) = w.upgrade() {
                        a.set_resyncing(text.is_some());
                        a.set_resync_progress(text.unwrap_or_default().into());
                    }
                });
            };
            if let Some(total) = dev_total {
                // Dev runs never touch the store: just walk the progress text over the
                // rows, as the real path does.
                post(Some(settings_accounts::progress_text(0, total)));
                for done in 1..=total {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    post(Some(settings_accounts::progress_text(done, total)));
                }
                post(None);
                return;
            }
            let results = resync::resync_all(&ids, &resync::RealResyncEnv, &|done, total| {
                post(Some(settings_accounts::progress_text(done, total)));
            });
            {
                let mut f = RESYNC_FAILURES.lock().unwrap_or_else(|e| e.into_inner());
                for (id, outcome) in &results {
                    if let resync::ResyncOutcome::Failed(msg) = outcome {
                        f.insert(id.clone(), msg.clone());
                    }
                }
            }
            post(None);
            // One pass over the accounts that re-synced (macOS `refreshAll(only: refreshable)`);
            // the failures above reach their cards through apply_output.
            let ok: Vec<String> = results
                .into_iter()
                .filter(|(_, o)| !matches!(o, resync::ResyncOutcome::Failed(_)))
                .map(|(id, _)| id)
                .collect();
            let _ = tx.send(if ok.is_empty() { refresh::Nudge::Local } else { refresh::Nudge::Some(ok) });
        });
    });
    let w = app.as_weak();
    app.on_resync_account(move |id| {
        let id = id.to_string();
        if dev_run() {
            eprintln!("resync {id}: skipped in a dev run");
            return;
        }
        let (w, tx) = (w.clone(), nudge_tx.clone());
        std::thread::spawn(move || {
            match resync::resync(&id, &resync::RealResyncEnv) {
                // Refresh just this card (macOS `refreshAll(only: [id])`).
                resync::ResyncOutcome::Updated | resync::ResyncOutcome::Refreshed => {
                    let _ = tx.send(refresh::Nudge::One(id.clone()));
                }
                resync::ResyncOutcome::Failed(msg) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        patch_row(&w, &id, |r| r.error = msg.as_str().into());
                    });
                }
            }
        });
    });
}

/// Dev-only: `CLAUDE_DASHBOARD_FAKE_ROWS=1` seeds sample cards (no network): a pinned
/// account, a Fable account, an extension account, and an expired one of each kind.
fn fake_rows(now: f64) -> refresh::RefreshOutput {
    use claude_dashboard_core::account_kind::AccountKind;
    use claude_dashboard_core::model::{AccountPlan, AccountStatus, Browser};
    use claude_dashboard_core::rows::{DisplayRow, WindowView};
    let win = |u: f64, resets_in: f64| WindowView { utilization: u, resets_at_unix: Some(now + resets_in), is_limited: false };
    let mk = |id: &str, name: &str, plan, u5: f64, u7: f64, burn: Option<f64>| DisplayRow {
        is_active_claude_code: name == "bob",
        account_id: id.into(),
        name: name.into(),
        email: Some(format!("{name}@example.com")),
        plan,
        status: AccountStatus::Active,
        five_hour: Some(win(u5, 7200.0)),
        seven_day: Some(win(u7, 259200.0)),
        fable: None,
        peak_utilization: u5.max(u7),
        burn_projected_seconds: burn,
        burn_projected_seconds_7d: None,
        burn_projected_seconds_fable: None,
        is_extension_sourced: false,
        error: None,
        last_synced_unix: Some(now),
        is_pinned: name == "alice",
        kind_label: AccountKind::Scanned { browser: Browser::Chrome, profile: "Default".into() }.settings_label(),
        expired_hint: String::new(),
    };
    let expired = |id: &str, name: &str, kind: AccountKind| DisplayRow {
        status: AccountStatus::Expired,
        five_hour: None,
        seven_day: None,
        peak_utilization: 0.0,
        is_extension_sourced: matches!(kind, AccountKind::Extension { .. }),
        kind_label: kind.settings_label(),
        expired_hint: kind.expired_hint(),
        ..mk(id, name, AccountPlan::Pro, 0.0, 0.0, None)
    };
    let mut bob = mk("f2", "bob", AccountPlan::Max5x, 64.0, 48.0, Some(3600.0));
    bob.fable = Some(win(40.0, 259200.0));
    bob.burn_projected_seconds_fable = Some(5400.0);
    bob.is_extension_sourced = true;
    bob.kind_label = AccountKind::Extension { browser: Browser::Edge }.settings_label();
    let mut carol = mk("f3", "carol", AccountPlan::Max20x, 100.0, 91.0, Some(600.0));
    if let Some(w) = carol.five_hour.as_mut() {
        w.is_limited = true;
    }
    // Flames: alice high critical (7d 60%, 10 hours left), bob critical (7d 55%, 25 hours left).
    let mut alice = mk("f1", "alice", AccountPlan::Pro, 12.0, 60.0, None);
    alice.seven_day = Some(win(60.0, 10.0 * 3600.0));
    bob.seven_day = Some(win(55.0, 25.0 * 3600.0));
    let rows = vec![
        alice,
        bob,
        carol,
        expired("f4", "dave", AccountKind::PastedKey),
        expired("f5", "erin", AccountKind::Scanned { browser: Browser::Edge, profile: "Work".into() }),
    ];
    refresh::RefreshOutput { peak: 100.0, rows, candidates: Vec::new() }
}

/// Time after a focus-loss hide during which a tray click is treated as the
/// same click that stole focus (so it does not immediately re-open the flyout).
const REOPEN_GUARD: std::time::Duration = std::time::Duration::from_millis(350);
/// Grace period after showing before focus-loss can hide the flyout.
const SHOW_GRACE: std::time::Duration = std::time::Duration::from_millis(400);

#[derive(Default)]
struct FlyoutState {
    shown_at: Option<std::time::Instant>,
    hidden_at: Option<std::time::Instant>,
    styled: bool,
}

thread_local! {
    static FLYOUT: std::cell::RefCell<FlyoutState> = std::cell::RefCell::new(FlyoutState::default());
}

fn hide_flyout(pop: &PopoverWindow) {
    pop.set_confirming_quit(false);
    let _ = pop.hide();
    FLYOUT.with(|f| f.borrow_mut().hidden_at = Some(std::time::Instant::now()));
}

/// Left-click on the tray: show the flyout next to the tray icon, or hide it.
fn toggle_flyout(pop: &PopoverWindow, tray_rect: popover::Rect) {
    if pop.window().is_visible() {
        hide_flyout(pop);
        return;
    }
    let recently_hidden = FLYOUT.with(|f| {
        f.borrow().hidden_at.is_some_and(|t| t.elapsed() < REOPEN_GUARD)
    });
    if recently_hidden {
        return;
    }
    if pop.show().is_err() {
        return;
    }
    let first = FLYOUT.with(|f| !std::mem::replace(&mut f.borrow_mut().styled, true));
    if first {
        popover::style_flyout(pop.window());
        if !apply_mica(pop.window()) {
            pop.set_use_solid_background(true);
        }
    }
    let phys = pop.window().size();
    let size = popover::Size { width: phys.width as i32, height: phys.height as i32 };
    let origin = popover::popover_origin(tray_rect, size, popover::work_area_near(tray_rect));
    pop.window()
        .set_position(slint::PhysicalPosition::new(origin.x, origin.y));
    popover::focus_flyout(pop.window());
    FLYOUT.with(|f| f.borrow_mut().shown_at = Some(std::time::Instant::now()));
}

thread_local! {
    /// The Add Account wizard opener, for `show_main`.
    static OPEN_WIZARD: std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
    /// Mica is applied the first time the main window shows (it may start hidden).
    static MAIN_STYLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Shows the main window. Like macOS `MainWindow.onAppear`, a store with no accounts
/// opens the Add Account wizard over it.
fn show_main(app: &AppWindow) {
    let _ = app.show();
    app.window().set_minimized(false);
    if !MAIN_STYLED.with(|s| s.replace(true)) && !apply_mica(app.window()) {
        app.set_use_solid_background(true);
    }
    let no_accounts = !dev_run() && claude_dashboard_core::store::load_accounts().is_ok_and(|a| a.is_empty());
    if no_accounts {
        if let Some(open) = OPEN_WIZARD.with(|o| o.borrow().clone()) {
            open();
        }
    }
}

/// The tray menu's Quit confirmation: a system message box with the flyout prompt's text.
fn confirm_quit() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDOK, MB_ICONQUESTION, MB_OKCANCEL, MB_SETFOREGROUND};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (text, title) = (wide("Usage monitoring stops until you open the app again."), wide("Quit Claude Dashboard?"));
    let r = unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OKCANCEL | MB_ICONQUESTION | MB_SETFOREGROUND)
    };
    r == IDOK
}

/// Best-effort: bring the window to the foreground (needs the second instance's
/// `AllowSetForegroundWindow`, or we are already foreground-eligible).
fn raise_to_foreground(window: &slint::Window) {
    let binding = window.window_handle();
    let Ok(handle) = binding.window_handle() else {
        return;
    };
    if let RawWindowHandle::Win32(h) = handle.as_raw() {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(h.hwnd.get() as *mut std::ffi::c_void);
        }
    }
}

thread_local! {
    static UPDATE_STATE: std::cell::RefCell<updater::UpdateState> =
        const { std::cell::RefCell::new(updater::UpdateState::Idle) };
}

/// UI thread only: stores `state` and mirrors it into the Updates card.
fn show_update_state(app: &AppWindow, state: updater::UpdateState) {
    use updater::UpdateState as S;
    app.set_update_status(updater::status_text(&state).into());
    app.set_update_status_kind(updater::status_kind(&state));
    // The Latest version row stays once a check has found a release (macOS `latestVersion`).
    if let Some(v) = updater::latest_version(&state) {
        app.set_update_latest(v.into());
    }
    app.set_update_busy(matches!(state, S::Checking | S::Downloading | S::Installing));
    // Yellow flyout banner while an update downloads or installs (macOS MenuBarPopover).
    let banner = if matches!(state, S::Downloading | S::Installing) {
        updater::status_text(&state)
    } else {
        String::new()
    };
    if let Some(pop) = POPOVER.with(|p| p.borrow().as_ref().and_then(|w| w.upgrade())) {
        pop.set_update_banner(banner.into());
    }
    UPDATE_STATE.with(|s| *s.borrow_mut() = state);
}

/// Any thread: schedule `show_update_state` on the UI thread.
fn post_update_state(weak: &slint::Weak<AppWindow>, state: updater::UpdateState) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(a) = weak.upgrade() {
            show_update_state(&a, state);
        }
    });
}

/// Process-wide claim on the download+install pipeline: the manual Install
/// button and the background auto-update must never run two at once (two
/// concurrent msiexec would fail with error 1618).
static UPDATE_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Worker: download, hand off to the relauncher, then quit. Any failure is shown.
/// A second claimant does nothing; the claim is released on failure (on success the app quits).
fn download_and_apply(weak: &slint::Weak<AppWindow>, info: &claude_dashboard_core::update::UpdateInfo, exe: &std::path::Path) {
    use std::sync::atomic::Ordering;
    use updater::UpdateState as S;
    if UPDATE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    post_update_state(weak, S::Downloading);
    let result = updater::download(info, settings_general::APP_VERSION).and_then(|msi| {
        post_update_state(weak, S::Installing);
        updater::apply(&msi, exe)
    });
    match result {
        Ok(()) => {
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        }
        Err(m) => {
            UPDATE_IN_FLIGHT.store(false, Ordering::Release);
            post_update_state(weak, S::Failed(m));
        }
    }
}

/// Settings > General > Updates plus the daily background auto-update.
fn install_updates(app: &AppWindow) {
    use updater::UpdateState as S;
    let exe = std::env::current_exe().unwrap_or_default();
    let local_appdata = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default());
    let installed = updater::is_installed_copy(&exe, &local_appdata);
    eprintln!("updater: installed copy = {installed}");

    show_update_state(app, if installed { S::Idle } else { S::DevBuild(None) });
    // macOS debug builds disable Check for Updates; a dev build here cannot install either.
    app.set_update_release(installed);
    let w = app.as_weak();
    std::thread::spawn(move || {
        let on = settings_general::auto_update_enabled();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(a) = w.upgrade() {
                a.set_auto_update(on);
            }
        });
    });

    // Check for Updates checks and, when a newer release exists, installs it in one go
    // (macOS `checkNow(autoInstall: true)`).
    let (w, exe_c) = (app.as_weak(), exe.clone());
    app.on_check_updates(move || {
        if !installed {
            return;
        }
        if let Some(a) = w.upgrade() {
            show_update_state(&a, S::Checking);
        }
        let (w, exe) = (w.clone(), exe_c.clone());
        std::thread::spawn(move || match updater::check(settings_general::APP_VERSION) {
            Ok(Some(info)) => {
                post_update_state(&w, S::Available(info.clone()));
                download_and_apply(&w, &info, &exe);
            }
            Ok(None) => post_update_state(&w, S::UpToDate),
            Err(m) => post_update_state(&w, S::Failed(m)),
        });
    });

    let w = app.as_weak();
    app.on_set_auto_update(move |on| {
        if let Some(a) = w.upgrade() {
            a.set_auto_update(on);
        }
        settings_general::writer().submit(move || {
            if let Err(e) = settings_general::set_auto_update(on) {
                eprintln!("save auto update failed: {e}");
            }
        });
    });

    // Daily background check; never under smoke/fake runs (no network, no msiexec).
    if std::env::var_os("CLAUDE_DASHBOARD_SMOKE").is_some()
        || std::env::var_os("CLAUDE_DASHBOARD_FAKE_ROWS").is_some()
    {
        return;
    }
    let w = app.as_weak();
    std::thread::spawn(move || loop {
        let settings = claude_dashboard_core::settings::load();
        let disable = std::env::var("DISABLE_AUTOUPDATER").ok();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        if updater::auto_update_allowed(settings.auto_update, installed, disable.as_deref())
            && claude_dashboard_core::update::is_due(
                settings.last_auto_update_check_unix,
                now,
                claude_dashboard_core::update::CHECK_INTERVAL_S,
            )
        {
            if let Err(e) = claude_dashboard_core::settings::update(|s| {
                s.last_auto_update_check_unix = Some(now)
            }) {
                eprintln!("stamp update check failed: {e}");
            }
            match updater::check(settings_general::APP_VERSION) {
                Ok(Some(i)) => download_and_apply(&w, &i, &exe),
                Ok(None) => {
                    // Never overwrite a Downloading/Installing state.
                    if !UPDATE_IN_FLIGHT.load(std::sync::atomic::Ordering::Acquire) {
                        post_update_state(&w, S::UpToDate);
                    }
                }
                Err(e) => {
                    eprintln!("auto update check failed: {e}");
                    // The release exists but its MSI isn't uploaded yet: retry hourly.
                    if let Some(t) = updater::restamp_after(&e, now) {
                        if let Err(e) = claude_dashboard_core::settings::update(|s| {
                            s.last_auto_update_check_unix = Some(t)
                        }) {
                            eprintln!("restamp update check failed: {e}");
                        }
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3600));
    });
}

fn main() -> Result<(), slint::PlatformError> {
    let Some(_instance) = instance::acquire_single_instance() else {
        // Already running: ask that instance to refresh/show, then leave.
        // Let the running instance take focus when it shows its window.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
                windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY,
            );
        }
        pipe::send("show");
        return Ok(());
    };
    // Remove files the retired Run Command feature left behind (never under dev/smoke runs).
    if std::env::var_os("CLAUDE_DASHBOARD_FAKE_ROWS").is_none()
        && std::env::var_os("CLAUDE_DASHBOARD_SMOKE").is_none()
    {
        if let (Some(local), Some(roaming)) =
            (std::env::var_os("LOCALAPPDATA"), std::env::var_os("APPDATA"))
        {
            migration::run_launch_cleanup(local.as_ref(), roaming.as_ref());
        }
        // Keep 90 days of usage history (macOS prunes at launch too).
        if let Ok(mut log) = claude_dashboard_core::store::UsageLogStore::open() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            log.delete_older_than(now - 90.0 * 86400.0);
        }
    }
    let (nudge_tx, nudge_rx) = std::sync::mpsc::channel::<refresh::Nudge>();
    let tray_nudge = nudge_tx.clone();
    let pop_nudge = nudge_tx.clone();
    let clock_nudge = nudge_tx.clone();
    let ping_nudge = nudge_tx.clone();
    let cc_nudge = nudge_tx.clone();

    let dark = system_is_dark();
    // Add Account wizard (hidden until opened); its poll timer stops on close.
    let open_wizard = wizard::install(dark, nudge_tx.clone())?;
    let app = AppWindow::new()?;
    {
        let pipe_tx = nudge_tx.clone();
        let pipe_weak = app.as_weak();
        pipe::serve(move |msg| {
            let _ = pipe_tx.send(refresh::Nudge::All);
            if msg == pipe::PipeMsg::Show {
                let w = pipe_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(a) = w.upgrade() {
                        show_main(&a);
                        raise_to_foreground(a.window());
                    }
                });
            }
        });
    }
    {
        let open = open_wizard.clone();
        app.on_add_account(move || open());
    }
    // Settings > Accounts: store I/O (under the store lock) runs on a worker;
    // results come back via invoke_from_event_loop, then the list is refreshed.
    {
        let reload_muted = {
            let w = app.as_weak();
            move || {
                let w = w.clone();
                std::thread::spawn(move || {
                    let muted = settings_accounts::muted_sources();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(a) = w.upgrade() {
                            let items: Vec<slint::SharedString> =
                                muted.into_iter().map(Into::into).collect();
                            a.set_muted_sources(slint::ModelRc::new(slint::VecModel::from(items)));
                        }
                    });
                });
            }
        };
        reload_muted();
        let (reload, tx) = (reload_muted.clone(), nudge_tx.clone());
        // Settings > Accounts and the account page's "Remove Account…" share one path.
        let delete_w = app.as_weak();
        let delete = move |id: slint::SharedString| {
            let (reload, tx) = (reload.clone(), tx.clone());
            let id = id.to_string();
            if dev_run() {
                // Fake rows: drop the row locally and re-publish; the real store is left alone.
                let last = LAST_OUTPUT.with(|l| l.borrow_mut().take());
                if let Some((mut out, now)) = last {
                    out.rows.retain(|r| r.account_id != id);
                    out.peak = claude_dashboard_core::rows::peak_utilization(&out.rows);
                    let pop = POPOVER.with(|p| p.borrow().clone()).unwrap_or_default();
                    apply_output(&delete_w, &pop, &out, now);
                }
                return;
            }
            std::thread::spawn(move || {
                match settings_accounts::delete_account(&id) {
                    Ok(_) => {
                        // No refresh token outlives its account (macOS `forgetClaudeCodeLogins`).
                        if let Some(s) = cc_state::switcher() {
                            s.forget(&id);
                        }
                        let _ = tx.send(refresh::Nudge::All);
                    }
                    Err(e) => eprintln!("delete account failed: {e}"),
                }
                let _ = slint::invoke_from_event_loop(reload);
            });
        };
        app.on_delete_account(delete.clone());
        app.on_remove_account(delete);
        let reload = reload_muted;
        app.on_unmute(move |id| {
            let reload = reload.clone();
            let id = id.to_string();
            std::thread::spawn(move || {
                if let Err(e) = settings_accounts::unmute(&id) {
                    eprintln!("unmute failed: {e}");
                }
                let _ = slint::invoke_from_event_loop(reload);
            });
        });
    }
    // Settings > General: settings/registry I/O on workers, results back on the UI thread.
    {
        app.set_help_topics(help::topic_model());
        // Reading a Card's sample ring (macOS: 5h at 62%, 2h15m left, a rabbit).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        app.set_help_sample(model::sample_window(62.0, 8100.0, 18000.0, "\u{1F407}", now));
        app.set_app_name(settings_general::APP_NAME.into());
        app.set_app_version(settings_general::APP_VERSION.into());
        app.set_release_date(settings_general::format_release_date(settings_general::RELEASE_DATE).into());
        {
            let tips = app.global::<TipStore>();
            tips.set_tips(tips::model());
            let w = app.as_weak();
            tips.on_more_help(move |topic| {
                if let Some(a) = w.upgrade() {
                    a.invoke_open_help(topic);
                }
            });
        }
        let w = app.as_weak();
        std::thread::spawn(move || {
            let (enabled, minutes) = settings_general::current_auto_refresh();
            let launch = settings_general::launch_is_enabled();
            let auto_ping = settings_general::auto_ping_enabled();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(a) = w.upgrade() {
                    a.set_auto_refresh_enabled(enabled);
                    a.set_auto_refresh_minutes(minutes as i32);
                    a.set_launch_at_startup(launch);
                    a.set_auto_ping(auto_ping);
                }
            });
        });
        // Auto Ping: turning it on refreshes now, so idle accounts are pinged without
        // waiting for the interval (macOS `setAutoPingEnabled`).
        let tx = nudge_tx.clone();
        let w = app.as_weak();
        app.on_set_auto_ping(move |on| {
            if let Some(a) = w.upgrade() {
                a.set_auto_ping(on);
            }
            let tx = tx.clone();
            settings_general::writer().submit(move || match settings_general::set_auto_ping(on) {
                Ok(()) if on => {
                    let _ = tx.send(refresh::Nudge::All);
                }
                Ok(()) => {}
                Err(e) => eprintln!("save auto ping failed: {e}"),
            });
        });
        let tx = nudge_tx.clone();
        let w = app.as_weak();
        app.on_set_auto_refresh_enabled(move |on| {
            let tx = tx.clone();
            if let Some(a) = w.upgrade() {
                a.set_auto_refresh_enabled(on);
            }
            settings_general::writer().submit(move || match settings_general::set_auto_refresh_enabled(on) {
                // Reschedule: the loop re-reads the interval and restarts its wait; no fetch.
                Ok(()) => {
                    let _ = tx.send(refresh::Nudge::Reschedule);
                }
                Err(e) => eprintln!("save auto refresh failed: {e}"),
            });
        });
        let tx = nudge_tx.clone();
        let w = app.as_weak();
        app.on_set_auto_refresh_minutes(move |minutes| {
            let tx = tx.clone();
            let minutes = minutes.clamp(1, 60);
            if let Some(a) = w.upgrade() {
                a.set_auto_refresh_minutes(minutes);
            }
            settings_general::writer().submit(move || match settings_general::set_auto_refresh_minutes(minutes as u64) {
                Ok(()) => {
                    let _ = tx.send(refresh::Nudge::Reschedule);
                }
                Err(e) => eprintln!("save auto refresh failed: {e}"),
            });
        });
        let tx = nudge_tx.clone();
        let w = app.as_weak();
        app.on_reset_all_settings(move || {
            let (tx, w) = (tx.clone(), w.clone());
            settings_general::writer().submit(move || match settings_general::reset_all() {
                Ok(state) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(a) = w.upgrade() {
                            a.set_auto_refresh_enabled(state.auto_refresh_enabled);
                            a.set_auto_refresh_minutes(state.auto_refresh_minutes as i32);
                            a.set_auto_update(state.auto_update);
                            a.set_auto_ping(state.auto_ping);
                        }
                        refresh_auto_switch_ui();
                    });
                    let _ = tx.send(refresh::Nudge::Reschedule);
                    // macOS `resetSettingsToDefaults` runs auto switch right away.
                    let _ = tx.send(refresh::Nudge::Local);
                }
                Err(e) => eprintln!("reset settings failed: {e}"),
            });
        });
        let w = app.as_weak();
        app.on_set_launch_at_startup(move |on| {
            let w = w.clone();
            settings_general::writer().submit(move || {
                if let Err(e) = settings_general::set_launch_at_startup(on) {
                    eprintln!("launch at startup failed: {e}");
                }
                // Re-read the real state so the toggle reflects what stuck.
                let actual = settings_general::launch_is_enabled();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(a) = w.upgrade() {
                        a.set_launch_at_startup(actual);
                    }
                });
            });
        });
    }
    install_card_actions(&app, nudge_tx.clone());
    // Re-sync button: same nudge the reload pipe sends (refreshes all accounts).
    install_updates(&app);
    history::install(&app);
    overview::install(&app);
    {
        // The account page's Refresh: this account only (macOS `refreshAll(only:)`).
        let (tx, w) = (nudge_tx.clone(), app.as_weak());
        app.on_refresh_account(move |id| {
            if dev_run() {
                // Fake rows: hold the Refresh button busy for a moment, then re-publish the seeded rows.
                begin_refresh(&w, &POPOVER.with(|p| p.borrow().clone()).unwrap_or_default());
                let w = w.clone();
                slint::Timer::single_shot(std::time::Duration::from_millis(1200), move || {
                    let last = LAST_OUTPUT.with(|l| l.borrow_mut().take());
                    if let Some((out, now)) = last {
                        let pop = POPOVER.with(|p| p.borrow().clone()).unwrap_or_default();
                        apply_output(&w, &pop, &out, now);
                    }
                });
                return;
            }
            let _ = tx.send(refresh::Nudge::One(id.to_string()));
        });
    }
    {
        // The account page's Ping: ping, then refresh that account only if the ping ran
        // (macOS `pingNow`).
        let tx = nudge_tx.clone();
        app.on_ping_account(move |id| {
            let id = id.to_string();
            if dev_run() {
                eprintln!("ping {id}: skipped in a dev run");
                return;
            }
            let tx = tx.clone();
            std::thread::spawn(move || {
                let Some(target) = ping_target(&id) else { return };
                if ping::run_ping(&target, claude_dashboard_core::ping_log::PingTrigger::Manual).is_some() {
                    let _ = tx.send(refresh::Nudge::One(id));
                }
            });
        });
        // Tools > Ping Log: load on open and on Refresh; Clear All also forgets the captions.
        // Tools > Switch Log: load on open and on Refresh.
        let w = app.as_weak();
        app.on_switch_log_reload(move || load_switch_log(&w));
        let w = app.as_weak();
        app.on_switch_log_clear(move || {
            let w = w.clone();
            std::thread::spawn(move || {
                if !dev_run() {
                    if let Ok(log) = claude_dashboard_core::auto_switch_log::AutoSwitchLogStore::open() {
                        log.clear();
                    }
                }
                let _ = slint::invoke_from_event_loop(move || load_switch_log(&w));
            });
        });
        let w = app.as_weak();
        app.on_ping_log_reload(move || load_ping_log(&w));
        let w = app.as_weak();
        app.on_ping_log_clear(move || {
            let w = w.clone();
            std::thread::spawn(move || {
                if !dev_run() {
                    if let Ok(mut log) = claude_dashboard_core::ping_log::PingLogStore::open() {
                        log.clear();
                    }
                }
                ping::controller().clear_latest();
                let _ = slint::invoke_from_event_loop(move || {
                    refresh_ping_ui(&w);
                    load_ping_log(&w);
                });
            });
        });
        let w = app.as_weak();
        ping::set_on_change(move || {
            let w = w.clone();
            let _ = slint::invoke_from_event_loop(move || refresh_ping_ui(&w));
        });
    }
    app.on_resync(move || {
        let _ = nudge_tx.send(refresh::Nudge::All);
    });
    app.global::<Theme>().set_dark(dark);
    OPEN_WIZARD.with(|o| *o.borrow_mut() = Some(open_wizard.clone()));
    // The Claude Code switcher reads and writes Claude Code's own files: never in dev runs.
    cc_state::init(!dev_run());
    MAIN_APP.with(|m| *m.borrow_mut() = Some(app.as_weak()));
    let _ = CC_NUDGE.set(cc_nudge);
    app.on_switch_claude_code(|id| cc_request(id.to_string()));
    app.on_toggle_home(|id| toggle_home(id.as_str()));
    // Settings > General > Auto Switch and Critical Accounts.
    {
        let hours: Vec<slint::SharedString> = (0..24).map(|h| format!("{h:02}:00").into()).collect();
        app.set_quiet_hour_names(slint::ModelRc::new(slint::VecModel::from(hours)));
        let zones: Vec<slint::SharedString> = claude_dashboard_core::critical::time_zone_names().into_iter().map(Into::into).collect();
        app.set_time_zone_names(slint::ModelRc::new(slint::VecModel::from(zones)));
        sync_switch_settings_ui(&app);
    }
    app.on_set_switch_setting(|key, value| {
        if key == "on" {
            set_auto_switch_enabled(value != 0);
            return;
        }
        let _ = claude_dashboard_core::settings::update(|s| settings_general::apply_switch_setting(s, key.as_str(), value as i64));
        refresh_auto_switch_ui();
    });
    app.on_set_critical_zone(|zone| {
        let _ = claude_dashboard_core::settings::update(|s| settings_general::apply_critical_zone(s, zone.as_str()));
        refresh_auto_switch_ui();
    });
    // X: the header button, its history overlay, and the hourly estimate (never in dev runs,
    // which keep the default).
    show_ratio_value(&app, &ratio::current());
    let w = app.as_weak();
    app.on_ratio_load(move || load_ratio_chart(&w));
    let w = app.as_weak();
    app.on_ratio_hover_at(move |f| {
        if let Some(a) = w.upgrade() {
            ratio_hover(&a, f);
        }
    });
    if !dev_run() {
        let w = app.as_weak();
        ratio::spawn(move |v| {
            let w = w.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(a) = w.upgrade() {
                    show_ratio_value(&a, &v);
                }
                // The flames depend on X.
                refresh_auto_switch_ui();
            });
        });
    }
    app.on_cc_prompt_ok(cc_ok);
    app.on_cc_prompt_cancel(cc_cancel);
    // macOS opens the window by itself only on the very first launch (`hasLaunchedBefore`);
    // later launches, launch at startup included, start in the tray. Dev runs always show it.
    let first_launch = !dev_run() && !claude_dashboard_core::settings::load().has_launched_before;
    if first_launch {
        let _ = claude_dashboard_core::settings::update(|s| s.has_launched_before = true);
    }
    if first_launch || dev_run() {
        show_main(&app);
    }

    // Tray flyout (created hidden; shown on tray left-click).
    let popover = PopoverWindow::new()?;
    popover.global::<Theme>().set_dark(dark);
    popover.set_app_version(settings_general::APP_VERSION.into());
    POPOVER.with(|p| *p.borrow_mut() = Some(popover.as_weak()));
    popover.on_switch_claude_code(|id| cc_request(id.to_string()));
    popover.on_toggle_home(|id| toggle_home(id.as_str()));
    popover.on_set_auto_switch(set_auto_switch_enabled);
    popover.on_cc_prompt_ok(cc_ok);
    popover.on_cc_prompt_cancel(cc_cancel);
    popover.on_refresh(move || {
        let _ = pop_nudge.send(refresh::Nudge::All);
    });
    popover.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
    {
        let (pop_w, open) = (popover.as_weak(), open_wizard.clone());
        popover.on_add_account(move || {
            if let Some(p) = pop_w.upgrade() {
                hide_flyout(&p);
            }
            open();
        });
    }
    // Header buttons and card actions: hide the flyout, then drive the main window.
    {
        let open_main = |app_w: slint::Weak<AppWindow>, pop_w: slint::Weak<PopoverWindow>| {
            move |f: Box<dyn FnOnce(&AppWindow)>| {
                if let Some(p) = pop_w.upgrade() {
                    hide_flyout(&p);
                }
                if let Some(a) = app_w.upgrade() {
                    f(&a);
                    show_main(&a);
                    raise_to_foreground(a.window());
                }
            }
        };
        let go = open_main(app.as_weak(), popover.as_weak());
        popover.on_expand({
            let go = go.clone();
            move || go(Box::new(|_| {}))
        });
        popover.on_open_help({
            let go = go.clone();
            move || go(Box::new(|a| a.invoke_open_help("".into())))
        });
        popover.on_open_settings({
            let go = go.clone();
            move || go(Box::new(|a| a.set_selection(Pane::General)))
        });
        popover.on_open_account_window({
            let go = go.clone();
            move |id, win| {
                let (id, win) = (id.to_string(), win.to_string());
                go(Box::new(move |a| {
                    a.set_selected_account_id(id.clone().into());
                    a.set_selection(Pane::Account);
                    a.set_requested_window(win.clone().into());
                    a.invoke_open_account_window(id.into(), win.into());
                }))
            }
        });
        let app_w = app.as_weak();
        popover.on_toggle_pin(move |id, pinned| {
            if let Some(a) = app_w.upgrade() {
                a.invoke_toggle_pin(id, pinned);
            }
        });
        let app_w = app.as_weak();
        popover.on_resync_account(move |id| {
            if let Some(a) = app_w.upgrade() {
                a.invoke_resync_account(id);
            }
        });
    }
    // Hide on focus loss (Slint has no focus-out callback for a Window).
    let focus_timer = slint::Timer::default();
    {
        let pop_w = popover.as_weak();
        focus_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(150),
            move || {
                let Some(p) = pop_w.upgrade() else { return };
                let settled = FLYOUT.with(|f| {
                    f.borrow().shown_at.is_some_and(|t| t.elapsed() > SHOW_GRACE)
                });
                if p.window().is_visible() && settled && popover::lost_focus(p.window()) {
                    hide_flyout(&p);
                }
            },
        );
    }

    let _tray = {
        let pop_w = popover.as_weak();
        let (open_app_w, open_pop_w) = (app.as_weak(), popover.as_weak());
        let actions = tray::TrayActions {
            toggle_popover: Box::new(move |rect| {
                let pop_w = pop_w.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(p) = pop_w.upgrade() {
                        toggle_flyout(&p, rect);
                    }
                });
            }),
            open_dashboard: Box::new(move || {
                let (a, p) = (open_app_w.clone(), open_pop_w.clone());
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(p) = p.upgrade() {
                        hide_flyout(&p);
                    }
                    if let Some(a) = a.upgrade() {
                        show_main(&a);
                        raise_to_foreground(a.window());
                    }
                });
            }),
            refresh: Box::new(move || {
                let _ = tray_nudge.send(refresh::Nudge::All);
            }),
            // Asks first, with the flyout's Quit prompt text (macOS has a confirmed Quit only).
            quit: Box::new(|| {
                let _ = slint::invoke_from_event_loop(|| {
                    if confirm_quit() {
                        let _ = slint::quit_event_loop();
                    }
                });
            }),
        };
        match tray::install_tray(None, &model::tray_tooltip(None, 0.0), actions) {
            Ok(h) => Some(h),
            Err(e) => {
                eprintln!("tray unavailable: {e}");
                None
            }
        }
    };

    // Smoke path: exit on its own so launches can be verified unattended.
    let _smoke = std::env::var_os("CLAUDE_DASHBOARD_SMOKE").map(|_| {
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(1200),
            || {
                let _ = slint::quit_event_loop();
            },
        );
        timer
    });

    // KNOWN DEV-PATH ANOMALY (not root-caused, production unaffected): under
    // FAKE_ROWS a second dark framed empty window has been observed besides the
    // main window; it does not appear on the normal refresh-loop path. The only
    // difference is apply_output running synchronously here, before the event
    // loop starts, instead of via invoke_from_event_loop. The popover is
    // no-frame and only show()n by toggle_flyout, so a pre-loop Slint window
    // realisation is suspected. Left as-is.
    if std::env::var_os("CLAUDE_DASHBOARD_FAKE_ROWS").is_some() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        apply_output(&app.as_weak(), &popover.as_weak(), &fake_rows(now), now);
        history::seed_fake_history();
    } else {
        // Each account page's latest ping, from the Ping Log.
        ping::load_latest();
        spawn_refresh_loop(nudge_rx, ping_nudge, app.as_weak(), popover.as_weak());
    }
    let _clock = start_clock(app.as_weak(), popover.as_weak(), clock_nudge);
    // Closing the main window hides it; the app lives in the tray and quits
    // only via the tray/flyout "Quit" (or the smoke timer) -> quit_event_loop.
    // Like macOS dropping the window's views, it reopens on the Dashboard.
    let close_w = app.as_weak();
    app.window().on_close_requested(move || {
        if let Some(a) = close_w.upgrade() {
            a.set_selection(Pane::Dashboard);
        }
        slint::CloseRequestResponse::HideWindow
    });
    // Hidden windows do not end this loop; only quit_event_loop does.
    // Dropping `_tray` on return removes the icon.
    let result = slint::run_event_loop_until_quit();
    drop(focus_timer);
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn fake_rows_cover_every_card_state() {
        // alice (pinned), bob (extension, Fable), carol (100%), dave and erin (expired).
        let out = super::fake_rows(1_000_000.0);
        assert_eq!(out.rows.len(), 5);
        assert_eq!(out.rows.iter().filter(|r| r.status == claude_dashboard_core::model::AccountStatus::Expired).count(), 2);
        let params = claude_dashboard_core::critical::parameters_from(&claude_dashboard_core::settings::Settings::default(), 16.0);
        let level = |i: usize| crate::auto_switch::card_fields(&out.rows[i], None, None, true, &params, 1_000_000.0).critical_level;
        assert_eq!((level(0), level(1)), (2, 1), "alice high critical, bob critical");
    }
}
