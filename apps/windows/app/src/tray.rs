//! System-tray icon: a ring filled to the peak utilization (colour and arc
//! both from `core`), plus the left-click / context-menu wiring.

use claude_dashboard_core::colors::usage_color;
use claude_dashboard_core::geometry::{progress_arc, ring_center, ring_radius};
use tiny_skia::{LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub const ICON_SIZE: u32 = 32;
const LINE_WIDTH: f64 = 6.0;
/// Polyline resolution for the arc (tiny-skia has no SVG-style arc command).
const ARC_STEPS_PER_TURN: f64 = 96.0;

/// Straight (non-premultiplied) RGBA8 image, as `tray_icon::Icon::from_rgba` wants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

fn stroke_arc(pixmap: &mut Pixmap, start: f64, end: f64, rgba: (u8, u8, u8, u8)) {
    let c = ring_center(ICON_SIZE as f64);
    let r = ring_radius(ICON_SIZE as f64, LINE_WIDTH);
    let steps = (((end - start).abs() / std::f64::consts::TAU) * ARC_STEPS_PER_TURN)
        .ceil()
        .max(1.0) as u32;
    let mut pb = PathBuilder::new();
    for i in 0..=steps {
        let a = start + (end - start) * (i as f64 / steps as f64);
        let (x, y) = ((c + r * a.cos()) as f32, (c + r * a.sin()) as f32);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgba.0, rgba.1, rgba.2, rgba.3);
    paint.anti_alias = true;
    let stroke = Stroke { width: LINE_WIDTH as f32, line_cap: LineCap::Round, ..Stroke::default() };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// Ring gauge filled to `peak_utilization` (percent), coloured by
/// `core::colors::usage_color`, over a faint full-circle track.
pub fn render_tray_icon(peak_utilization: f64) -> RgbaImage {
    let mut pixmap = Pixmap::new(ICON_SIZE, ICON_SIZE).expect("non-zero icon size");
    // Track: full ring in neutral grey (visible on light and dark taskbars).
    stroke_arc(&mut pixmap, 0.0, std::f64::consts::TAU * 0.999, (140, 140, 140, 110));
    let (start, end) = progress_arc(peak_utilization);
    if end > start {
        let c = usage_color(peak_utilization);
        let ch = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        stroke_arc(&mut pixmap, start, end, (ch(c.r), ch(c.g), ch(c.b), 255));
    }
    let mut data = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for p in pixmap.pixels() {
        let d = p.demultiply();
        data.extend_from_slice(&[d.red(), d.green(), d.blue(), d.alpha()]);
    }
    RgbaImage { width: ICON_SIZE, height: ICON_SIZE, data }
}

/// What the tray does; every callback may run on any thread (the caller
/// marshals to the UI thread with `slint::invoke_from_event_loop`).
pub struct TrayActions {
    /// Left click; receives the tray icon rect in physical screen pixels.
    pub toggle_popover: Box<dyn Fn(crate::popover::Rect) + Send + Sync>,
    pub open_dashboard: Box<dyn Fn() + Send + Sync>,
    pub refresh: Box<dyn Fn() + Send + Sync>,
    pub quit: Box<dyn Fn() + Send + Sync>,
}

thread_local! {
    static TRAY: std::cell::RefCell<Option<tray_icon::TrayIcon>> = const { std::cell::RefCell::new(None) };
}

/// Keeps the tray icon alive; dropping it removes the icon from the tray.
pub struct TrayHandle(());

impl Drop for TrayHandle {
    fn drop(&mut self) {
        TRAY.with(|t| t.borrow_mut().take());
    }
}

fn to_icon(img: &RgbaImage) -> Option<tray_icon::Icon> {
    tray_icon::Icon::from_rgba(img.data.clone(), img.width, img.height).ok()
}

/// Redraw the tray ring for a new peak. Must run on the thread that called
/// `install_tray` (the UI thread).
pub fn update_peak(peak: f64) {
    TRAY.with(|t| {
        if let (Some(tray), Some(icon)) = (t.borrow().as_ref(), to_icon(&render_tray_icon(peak))) {
            let _ = tray.set_icon(Some(icon));
            let _ = tray.set_tooltip(Some(format!("Claude Dashboard - peak {peak:.0}%")));
        }
    });
}

/// Install the tray icon (call on the UI thread, before the event loop runs).
/// Left click toggles the popover; right click opens Open Dashboard /
/// Refresh / Quit.
pub fn install_tray(peak: f64, actions: TrayActions) -> Result<TrayHandle, String> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let open = MenuItem::with_id("open", "Open Dashboard", true, None);
    let refresh = MenuItem::with_id("refresh", "Refresh", true, None);
    let quit = MenuItem::with_id("quit", "Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&open, &refresh, &quit]).map_err(|e| e.to_string())?;

    let icon = to_icon(&render_tray_icon(peak)).ok_or("bad tray icon")?;
    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip(format!("Claude Dashboard - peak {peak:.0}%"))
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())?;
    TRAY.with(|t| *t.borrow_mut() = Some(tray));

    let toggle = actions.toggle_popover;
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            rect,
            ..
        } = e
        {
            toggle(crate::popover::Rect {
                x: rect.position.x.round() as i32,
                y: rect.position.y.round() as i32,
                width: rect.size.width as i32,
                height: rect.size.height as i32,
            });
        }
    }));
    let (open_cb, refresh_cb, quit_cb) = (actions.open_dashboard, actions.refresh, actions.quit);
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| match e.id.0.as_str() {
        "open" => open_cb(),
        "refresh" => refresh_cb(),
        "quit" => quit_cb(),
        _ => {}
    }));
    Ok(TrayHandle(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_is_deterministic_and_non_empty() {
        let a = render_tray_icon(42.0);
        let b = render_tray_icon(42.0);
        assert_eq!(a, b);
        assert_eq!((a.width, a.height), (ICON_SIZE, ICON_SIZE));
        assert_eq!(a.data.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(a.data.chunks(4).any(|p| p[3] > 0), "fully transparent");
    }

    #[test]
    fn peak_zero_and_hundred_differ() {
        assert_ne!(render_tray_icon(0.0), render_tray_icon(100.0));
        assert_ne!(render_tray_icon(30.0), render_tray_icon(80.0));
    }
}
