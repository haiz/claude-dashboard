//! Flyout placement: pure geometry (no windowing calls) so it is unit-testable.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.width
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }
}

/// Gap between the tray icon and the flyout.
const GAP: i32 = 8;

/// Top-left of the popover: centred on the tray icon horizontally, above it
/// when the tray is in the lower half of the work area (taskbar at bottom),
/// below it otherwise (taskbar at top); then clamped so the whole popover
/// lies inside `work_area`.
pub fn popover_origin(tray_rect: Rect, popover_size: Size, work_area: Rect) -> Point {
    let tray_cx = tray_rect.x + tray_rect.width / 2;
    let tray_cy = tray_rect.y + tray_rect.height / 2;
    let work_cy = work_area.y + work_area.height / 2;

    let x = tray_cx - popover_size.width / 2;
    let y = if tray_cy >= work_cy {
        tray_rect.y - popover_size.height - GAP
    } else {
        tray_rect.bottom() + GAP
    };
    Point {
        x: clamp_span(x, popover_size.width, work_area.x, work_area.right()),
        y: clamp_span(y, popover_size.height, work_area.y, work_area.bottom()),
    }
}

/// Start coordinate keeping `[start, start + len]` inside `[lo, hi]`; when the
/// span is larger than the range, pin to `lo` so the top-left stays visible.
fn clamp_span(start: i32, len: i32, lo: i32, hi: i32) -> i32 {
    start.min(hi - len).max(lo)
}

// ---- Win32 glue (not unit-tested): monitor work area, flyout window styling ----

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Work area (monitor minus taskbar) of the monitor nearest `tray_rect`.
pub fn work_area_near(tray_rect: Rect) -> Rect {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    let pt = POINT { x: tray_rect.x + tray_rect.width / 2, y: tray_rect.y + tray_rect.height / 2 };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let ok = unsafe {
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(mon, &mut info)
    };
    if ok == 0 {
        // Unknown monitor: treat a generous area around the tray as usable.
        return Rect { x: 0, y: 0, width: tray_rect.right().max(1920), height: tray_rect.bottom().max(1080) };
    }
    let w = info.rcWork;
    Rect { x: w.left, y: w.top, width: w.right - w.left, height: w.bottom - w.top }
}

fn hwnd_of(window: &slint::Window) -> Option<isize> {
    let binding = window.window_handle();
    let handle = binding.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// One-time styling of the flyout: rounded corners, hidden from the taskbar.
pub fn style_flyout(window: &slint::Window) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };
    let Some(hwnd) = hwnd_of(window) else { return };
    let hwnd = hwnd as *mut core::ffi::c_void;
    let round: i32 = 2; // DWMWCP_ROUND
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &round as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW as isize);
    }
}

/// Bring the flyout to the foreground so focus-loss can be detected.
pub fn focus_flyout(window: &slint::Window) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetForegroundWindow, SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE,
    };
    if let Some(hwnd) = hwnd_of(window) {
        let hwnd = hwnd as *mut core::ffi::c_void;
        unsafe {
            SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            SetForegroundWindow(hwnd);
        }
    }
}

/// True when the flyout window is no longer the foreground window.
pub fn lost_focus(window: &slint::Window) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    match hwnd_of(window) {
        Some(h) => unsafe { GetForegroundWindow() as isize != h },
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popover_origin_stays_within_work_area() {
        let size = Size { width: 340, height: 420 };
        // 1920x1080 screen; work areas model the taskbar on each edge.
        let work_areas = [
            Rect { x: 0, y: 0, width: 1920, height: 1040 },  // taskbar bottom
            Rect { x: 0, y: 40, width: 1920, height: 1040 }, // taskbar top
            Rect { x: 60, y: 0, width: 1860, height: 1080 }, // taskbar left
            Rect { x: 0, y: 0, width: 1860, height: 1080 },  // taskbar right
        ];
        let (tw, th) = (24, 24);
        for wa in work_areas {
            let corners = [
                (wa.x, wa.y),
                (wa.right() - tw, wa.y),
                (wa.x, wa.bottom() - th),
                (wa.right() - tw, wa.bottom() - th),
                (wa.x + wa.width / 2, wa.y + wa.height / 2),
                (1920 - tw, 1080 - th), // classic notification area, on the taskbar
                (0, 1040),
                (1920 - tw, 0),
            ];
            for (x, y) in corners {
                let tray = Rect { x, y, width: tw, height: th };
                let o = popover_origin(tray, size, wa);
                assert!(o.x >= wa.x, "left {o:?} {tray:?} {wa:?}");
                assert!(o.y >= wa.y, "top {o:?} {tray:?} {wa:?}");
                assert!(o.x + size.width <= wa.right(), "right {o:?} {tray:?} {wa:?}");
                assert!(o.y + size.height <= wa.bottom(), "bottom {o:?} {tray:?} {wa:?}");
            }
        }
    }

    #[test]
    fn popover_sits_above_tray_when_taskbar_at_bottom() {
        let wa = Rect { x: 0, y: 0, width: 1920, height: 1040 };
        let tray = Rect { x: 1800, y: 1048, width: 24, height: 24 };
        let size = Size { width: 340, height: 420 };
        let o = popover_origin(tray, size, wa);
        assert!(o.y + size.height <= tray.y);
    }

    #[test]
    fn popover_sits_below_tray_when_taskbar_at_top() {
        let wa = Rect { x: 0, y: 40, width: 1920, height: 1040 };
        let tray = Rect { x: 1800, y: 8, width: 24, height: 24 };
        let size = Size { width: 340, height: 420 };
        let o = popover_origin(tray, size, wa);
        assert!(o.y >= tray.bottom());
    }

    #[test]
    fn oversized_popover_pins_to_work_area_origin() {
        let wa = Rect { x: 10, y: 20, width: 200, height: 200 };
        let o = popover_origin(
            Rect { x: 100, y: 100, width: 16, height: 16 },
            Size { width: 340, height: 420 },
            wa,
        );
        assert_eq!(o, Point { x: 10, y: 20 });
    }
}
