#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;

mod chart;
mod chart_model;
mod help;
mod history;
mod overview;
mod instance;
mod migration;
mod model;
mod pin;
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
    weak: slint::Weak<AppWindow>,
    pop_weak: slint::Weak<PopoverWindow>,
) {
    use refresh::Nudge;
    std::thread::spawn(move || {
        let mut prev: Vec<claude_dashboard_core::rows::DisplayRow> = Vec::new();
        let mut tracker = claude_dashboard_core::burn_rate::BurnRateTracker::default();
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
                let only = match &job {
                    Nudge::One(id) => Some(id.clone()),
                    Nudge::All | Nudge::Reschedule => None,
                };
                // Spinners on until apply_output replaces the rows.
                let (w, pw, o) = (weak.clone(), pop_weak.clone(), only.clone());
                let _ = slint::invoke_from_event_loop(move || begin_refresh(&w, &pw, o.as_deref()));
                let mut out = match &job {
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
                    Nudge::One(id) => {
                        let mut out = refresh::refresh_one(id, &prev, now);
                        if out.rows != prev {
                            if let Some(i) = out.rows.iter().position(|r| &r.account_id == id) {
                                refresh::apply_projections(&mut tracker, &mut out.rows[i..=i], now);
                            }
                        }
                        out
                    }
                };
                out.peak = claude_dashboard_core::rows::peak_utilization(&out.rows);
                prev = out.rows.clone();
                let (weak, pop_weak) = (weak.clone(), pop_weak.clone());
                let _ = slint::invoke_from_event_loop(move || apply_output(&weak, &pop_weak, &out, now));
            }
            jobs = match refresh::loop_timeout(&claude_dashboard_core::settings::load()) {
                Some(t) => match rx.recv_timeout(t) {
                    Ok(n) => pending(n, &rx),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => vec![Nudge::All],
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                },
                // Auto refresh off: wait for a nudge only.
                None => match rx.recv() {
                    Ok(n) => pending(n, &rx),
                    Err(_) => return,
                },
            };
        }
    });
}

/// The first nudge plus whatever else is already queued, folded into the cycles to run.
fn pending(first: refresh::Nudge, rx: &std::sync::mpsc::Receiver<refresh::Nudge>) -> Vec<refresh::Nudge> {
    let mut all = vec![first];
    all.extend(rx.try_iter());
    refresh::coalesce(all)
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

/// UI thread: marks the non-expired cards of a refresh as loading, in the main window and
/// the flyout (macOS `refreshAll` skips expired accounts): every card for `only == None`,
/// else just that account's. Also raises `refreshing`; the next `apply_output` clears both.
fn begin_refresh(weak: &slint::Weak<AppWindow>, pop_weak: &slint::Weak<PopoverWindow>, only: Option<&str>) {
    use slint::Model;
    let mark = |rows: slint::ModelRc<UiRow>| {
        for i in 0..rows.row_count() {
            if let Some(mut r) = rows.row_data(i) {
                if !r.expired && only.is_none_or(|id| r.id == id) {
                    r.loading = true;
                    rows.set_row_data(i, r);
                }
            }
        }
    };
    if let Some(app) = weak.upgrade() {
        mark(app.get_account_rows());
        mark(app.get_pane_rows());
        app.set_refreshing(true);
    }
    if let Some(pop) = pop_weak.upgrade() {
        mark(pop.get_account_rows());
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
            Ok(()) => {
                let _ = tx.send(refresh::Nudge::All);
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
        // Dev runs: the scanned fake rows, the denominator the real path counts from the store.
        let dev_scanned = dev_run().then(|| {
            LAST_OUTPUT.with(|l| l.borrow().as_ref().map_or(0, |(out, _)| fake_scanned_total(&out.rows)))
        });
        app.set_resyncing(true);
        // Until the worker counts the scanned accounts.
        app.set_resync_progress("Re-syncing\u{2026}".into());
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
            if let Some(total) = dev_scanned {
                // Dev runs never touch the store: just walk the progress text over the
                // scanned rows, as the real path does.
                post(Some(settings_accounts::progress_text(0, total)));
                for done in 1..=total {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    post(Some(settings_accounts::progress_text(done, total)));
                }
                post(None);
                return;
            }
            post(Some(settings_accounts::progress_text(0, resync::scanned_total(&ids))));
            let results = resync::resync_all(&ids, &resync::RealResyncEnv, &|done, total| {
                post(Some(settings_accounts::progress_text(done, total)));
            });
            {
                let mut f = RESYNC_FAILURES.lock().unwrap_or_else(|e| e.into_inner());
                for (id, outcome) in results {
                    if let resync::ResyncOutcome::Failed(msg) = outcome {
                        f.insert(id, msg);
                    }
                }
            }
            post(None);
            let _ = tx.send(refresh::Nudge::All);
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
                resync::ResyncOutcome::Updated | resync::ResyncOutcome::Refreshed => {
                    let _ = tx.send(refresh::Nudge::All);
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

/// Dev-only: how many fake rows are scanned accounts (neither extension-fed nor pasted).
fn fake_scanned_total(rows: &[claude_dashboard_core::rows::DisplayRow]) -> usize {
    let pasted = claude_dashboard_core::account_kind::AccountKind::PastedKey.settings_label();
    rows.iter()
        .filter(|r| !r.is_extension_sourced && r.kind_label != pasted)
        .count()
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
    let rows = vec![
        mk("f1", "alice", AccountPlan::Pro, 12.0, 30.0, None),
        bob,
        carol,
        expired("f4", "dave", AccountKind::PastedKey),
        expired("f5", "erin", AccountKind::Scanned { browser: Browser::Edge, profile: "Work".into() }),
    ];
    refresh::RefreshOutput { peak: 100.0, rows }
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

fn show_main(app: &AppWindow) {
    let _ = app.show();
    app.window().set_minimized(false);
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
    app.set_update_status(updater::status_text(&state, settings_general::APP_VERSION).into());
    app.set_update_can_install(matches!(state, S::Available(_)));
    app.set_update_busy(matches!(state, S::Checking | S::Downloading | S::Installing));
    // Yellow flyout banner while an update downloads or installs (macOS MenuBarPopover).
    let banner = if matches!(state, S::Downloading | S::Installing) {
        updater::status_text(&state, settings_general::APP_VERSION)
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
    let w = app.as_weak();
    std::thread::spawn(move || {
        let on = settings_general::auto_update_enabled();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(a) = w.upgrade() {
                a.set_auto_update(on);
            }
        });
    });

    let w = app.as_weak();
    app.on_check_updates(move || {
        if let Some(a) = w.upgrade() {
            show_update_state(&a, S::Checking);
        }
        let w = w.clone();
        std::thread::spawn(move || {
            let state = match updater::check(settings_general::APP_VERSION) {
                Ok(Some(i)) if installed => S::Available(i),
                Ok(Some(i)) => S::DevBuild(Some(i)),
                Ok(None) if installed => S::UpToDate,
                Ok(None) => S::DevBuild(None),
                Err(m) => S::Failed(m),
            };
            post_update_state(&w, state);
        });
    });

    let (w, exe_i) = (app.as_weak(), exe.clone());
    app.on_install_update(move || {
        let state = UPDATE_STATE.with(|s| s.borrow().clone());
        let S::Available(info) = state else { return };
        if !installed {
            return;
        }
        // Clear can-install right now, on the UI thread, so a double click
        // cannot reach this handler twice with the state still Available.
        if let Some(a) = w.upgrade() {
            show_update_state(&a, S::Downloading);
        }
        let (w, exe) = (w.clone(), exe_i.clone());
        std::thread::spawn(move || download_and_apply(&w, &info, &exe));
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
    }
    let (nudge_tx, nudge_rx) = std::sync::mpsc::channel::<refresh::Nudge>();
    let tray_nudge = nudge_tx.clone();
    let pop_nudge = nudge_tx.clone();

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
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(a) = w.upgrade() {
                    a.set_auto_refresh_enabled(enabled);
                    a.set_auto_refresh_minutes(minutes as i32);
                    a.set_launch_at_startup(launch);
                }
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
                        }
                    });
                    let _ = tx.send(refresh::Nudge::Reschedule);
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
                // Fake rows: spin the card for a moment, then re-publish the seeded rows.
                begin_refresh(&w, &POPOVER.with(|p| p.borrow().clone()).unwrap_or_default(), Some(id.as_str()));
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
    app.on_resync(move || {
        let _ = nudge_tx.send(refresh::Nudge::All);
    });
    app.global::<Theme>().set_dark(dark);
    app.show()?;
    if !apply_mica(app.window()) {
        app.set_use_solid_background(true);
    }

    // Tray flyout (created hidden; shown on tray left-click).
    let popover = PopoverWindow::new()?;
    popover.global::<Theme>().set_dark(dark);
    popover.set_app_version(settings_general::APP_VERSION.into());
    POPOVER.with(|p| *p.borrow_mut() = Some(popover.as_weak()));
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
        popover.on_open_account({
            let go = go.clone();
            move |id| {
                let id = id.to_string();
                go(Box::new(move |a| {
                    a.set_selected_account_id(id.into());
                    a.set_selection(Pane::Account);
                }))
            }
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
                    }
                });
            }),
            refresh: Box::new(move || {
                let _ = tray_nudge.send(refresh::Nudge::All);
            }),
            quit: Box::new(|| {
                let _ = slint::invoke_from_event_loop(|| {
                    let _ = slint::quit_event_loop();
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
        spawn_refresh_loop(nudge_rx, app.as_weak(), popover.as_weak());
    }
    // Closing the main window hides it; the app lives in the tray and quits
    // only via the tray/flyout "Quit" (or the smoke timer) -> quit_event_loop.
    app.window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
    // Hidden windows do not end this loop; only quit_event_loop does.
    // Dropping `_tray` on return removes the icon.
    let result = slint::run_event_loop_until_quit();
    drop(focus_timer);
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn fake_resync_all_counts_scanned_rows_only() {
        // alice, carol and erin are scanned; bob is extension-fed, dave pasted.
        let out = super::fake_rows(1_000_000.0);
        assert_eq!(out.rows.len(), 5);
        assert_eq!(super::fake_scanned_total(&out.rows), 3);
    }
}
