use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;

mod instance;
mod pipe;
mod refresh;

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

const REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Background loop: refresh now, then every interval or whenever nudged.
fn spawn_refresh_loop(rx: std::sync::mpsc::Receiver<()>) {
    std::thread::spawn(move || {
        let mut prev: Vec<claude_dashboard_core::rows::DisplayRow> = Vec::new();
        loop {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let out = refresh::merge_errors(&prev, Ok(refresh::refresh_once(now)));
            prev = out.rows.clone();
            // Task 7 binds this to the UI model; for now just log the count.
            let _ = slint::invoke_from_event_loop(move || {
                eprintln!("refresh: {} rows, peak {:.0}%", out.rows.len(), out.peak);
            });
            match rx.recv_timeout(REFRESH_INTERVAL) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

fn main() -> Result<(), slint::PlatformError> {
    let Some(_instance) = instance::acquire_single_instance() else {
        // Already running: ask that instance to refresh/show, then leave.
        pipe::send("show");
        return Ok(());
    };
    let (nudge_tx, nudge_rx) = std::sync::mpsc::channel::<()>();
    pipe::serve_reload(move || {
        let _ = nudge_tx.send(());
    });

    let app = AppWindow::new()?;
    app.global::<Theme>().set_dark(system_is_dark());
    app.show()?;
    if !apply_mica(app.window()) {
        app.set_use_solid_background(true);
    }

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

    spawn_refresh_loop(nudge_rx);
    app.run()
}
