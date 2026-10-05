use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;

mod chart;
mod chart_model;
mod commands;
mod overview;
mod instance;
mod model;
mod pipe;
mod popover;
mod refresh;
mod settings_accounts;
mod settings_general;
mod setup;
mod runner;
mod shell;
#[cfg(test)]
mod testenv;
mod terminal;
mod tray;
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
    static LAST_PEAK: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

/// The single fan-out for one refresh result: main window rows, popover rows
/// and the tray icon (redrawn only when the peak changes). UI thread only.
fn apply_output(
    weak: &slint::Weak<AppWindow>,
    pop_weak: &slint::Weak<PopoverWindow>,
    out: &refresh::RefreshOutput,
    now: f64,
) {
    eprintln!("refresh: {} rows, peak {:.0}%", out.rows.len(), out.peak);
    let rows: Vec<UiRow> = out.rows.iter().map(|r| model::to_ui_row(r, now)).collect();
    if let Some(pop) = pop_weak.upgrade() {
        pop.set_account_rows(slint::ModelRc::new(slint::VecModel::from(rows.clone())));
    }
    if let Some(app) = weak.upgrade() {
        app.set_account_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
    }
    if LAST_PEAK.with(|p| p.replace(Some(out.peak))) != Some(out.peak) {
        tray::update_peak(out.peak);
    }
}

/// Background loop: refresh now, then every Auto Refresh interval (re-read each
/// cycle, so a settings change applies without restart) or whenever nudged.
fn spawn_refresh_loop(
    rx: std::sync::mpsc::Receiver<()>,
    weak: slint::Weak<AppWindow>,
    pop_weak: slint::Weak<PopoverWindow>,
) {
    std::thread::spawn(move || {
        let mut prev: Vec<claude_dashboard_core::rows::DisplayRow> = Vec::new();
        loop {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let out = refresh::merge_errors(&prev, refresh::refresh_once(now));
            prev = out.rows.clone();
            let (weak, pop_weak) = (weak.clone(), pop_weak.clone());
            let _ = slint::invoke_from_event_loop(move || apply_output(&weak, &pop_weak, &out, now));
            match rx.recv_timeout(refresh::loop_timeout(&claude_dashboard_core::settings::load())) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}


/// Dev-only: `CLAUDE_DASHBOARD_FAKE_ROWS=1` seeds sample cards (no network).
fn fake_rows(now: f64) -> refresh::RefreshOutput {
    use claude_dashboard_core::model::{AccountPlan, AccountStatus};
    use claude_dashboard_core::rows::{DisplayRow, WindowView};
    let mk = |id: &str, name: &str, plan, u5: f64, u7: f64, burn: Option<f64>| DisplayRow {
        account_id: id.into(),
        name: name.into(),
        email: Some(format!("{name}@example.com")),
        plan,
        status: AccountStatus::Active,
        five_hour: Some(WindowView { utilization: u5, resets_at_unix: Some(now + 7200.0), is_limited: false }),
        seven_day: Some(WindowView { utilization: u7, resets_at_unix: Some(now + 259200.0), is_limited: false }),
        fable: None,
        peak_utilization: u5.max(u7),
        burn_projected_seconds: burn,
        is_extension_sourced: false,
        error: None,
        last_synced_unix: Some(now),
        is_active_claude_code: false,
    };
    let rows = vec![
        mk("f1", "alice", AccountPlan::Pro, 12.0, 30.0, None),
        mk("f2", "bob", AccountPlan::Max5x, 64.0, 48.0, Some(3600.0)),
        mk("f3", "carol", AccountPlan::Max20x, 100.0, 91.0, Some(600.0)),
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

fn main() -> Result<(), slint::PlatformError> {
    let Some(_instance) = instance::acquire_single_instance() else {
        // Already running: ask that instance to refresh/show, then leave.
        pipe::send("show");
        return Ok(());
    };
    let (nudge_tx, nudge_rx) = std::sync::mpsc::channel::<()>();
    let pipe_tx = nudge_tx.clone();
    pipe::serve_reload(move || {
        let _ = pipe_tx.send(());
    });
    let tray_nudge = nudge_tx.clone();
    let pop_nudge = nudge_tx.clone();

    let dark = system_is_dark();
    // Add Account wizard (hidden until opened); its poll timer stops on close.
    let open_wizard = wizard::install(dark, nudge_tx.clone())?;
    let app = AppWindow::new()?;
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
        app.on_delete_account(move |id| {
            let (reload, tx) = (reload.clone(), tx.clone());
            let id = id.to_string();
            std::thread::spawn(move || {
                match settings_accounts::delete_account(&id) {
                    Ok(_) => {
                        let _ = tx.send(());
                    }
                    Err(e) => eprintln!("delete account failed: {e}"),
                }
                let _ = slint::invoke_from_event_loop(reload);
            });
        });
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
        app.set_app_name(settings_general::APP_NAME.into());
        app.set_app_version(settings_general::APP_VERSION.into());
        let w = app.as_weak();
        std::thread::spawn(move || {
            let secs = settings_general::current_auto_refresh() as i32;
            let launch = settings_general::launch_is_enabled();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(a) = w.upgrade() {
                    a.set_auto_refresh_seconds(secs);
                    a.set_launch_at_startup(launch);
                }
            });
        });
        let tx = nudge_tx.clone();
        let w = app.as_weak();
        app.on_set_auto_refresh(move |secs| {
            let (tx, w) = (tx.clone(), w.clone());
            if let Some(a) = w.upgrade() {
                a.set_auto_refresh_seconds(secs);
            }
            std::thread::spawn(move || {
                match settings_general::set_auto_refresh(secs.max(0) as u64) {
                    // Nudge: the loop wakes and re-reads the interval right away.
                    Ok(()) => {
                        let _ = tx.send(());
                    }
                    Err(e) => eprintln!("save auto refresh failed: {e}"),
                }
            });
        });
        let w = app.as_weak();
        app.on_set_launch_at_startup(move |on| {
            let w = w.clone();
            std::thread::spawn(move || {
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
    // Re-sync button: same nudge the reload pipe sends (refreshes all accounts).
    chart::install(&app);
    overview::install(&app);
    app.on_resync(move || {
        let _ = nudge_tx.send(());
    });
    app.global::<Theme>().set_dark(dark);
    app.show()?;
    if !apply_mica(app.window()) {
        app.set_use_solid_background(true);
    }

    // Tray flyout (created hidden; shown on tray left-click).
    let popover = PopoverWindow::new()?;
    popover.global::<Theme>().set_dark(dark);
    popover.on_refresh(move || {
        let _ = pop_nudge.send(());
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
    {
        let (app_w, pop_w) = (app.as_weak(), popover.as_weak());
        popover.on_expand(move || {
            if let Some(p) = pop_w.upgrade() {
                hide_flyout(&p);
            }
            if let Some(a) = app_w.upgrade() {
                show_main(&a);
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
                let _ = tray_nudge.send(());
            }),
            quit: Box::new(|| {
                let _ = slint::invoke_from_event_loop(|| {
                    let _ = slint::quit_event_loop();
                });
            }),
        };
        match tray::install_tray(0.0, actions) {
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
        chart::seed_fake_history();
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
