use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;

slint::include_modules!();

/// Apply Mica to the window; silently ignored where unsupported (Windows 10).
fn apply_mica(window: &slint::Window) {
    let binding = window.window_handle();
    let Ok(handle) = binding.window_handle() else {
        return;
    };
    if let RawWindowHandle::Win32(_) = handle.as_raw() {
        let _ = window_vibrancy::apply_mica(&handle, None);
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;
    app.show()?;
    apply_mica(app.window());

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

    app.run()
}
