//! Showing, hiding and positioning the floating widget window.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

const MARGIN: f64 = 12.0;

#[derive(Default)]
pub struct WidgetState {
    /// When the widget was last hidden because it lost focus. Clicking the tray
    /// icon / toolbar item steals focus first, so without this a "toggle" right
    /// after a blur-hide would immediately re-open the widget.
    last_blur_hide: Mutex<Option<Instant>>,
    /// Whether the window grows upwards (anchored to its bottom edge), which is
    /// the case when it was opened from a bottom taskbar / tray.
    anchored_bottom: Mutex<bool>,
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

pub fn on_blur(app: &AppHandle) {
    if let Some(w) = window(app) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
            *app.state::<WidgetState>().last_blur_hide.lock().unwrap() = Some(Instant::now());
        }
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.hide();
    }
}

pub fn toggle(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    let recently_hidden = app
        .state::<WidgetState>()
        .last_blur_hide
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if !recently_hidden {
        show(app);
    }
}

/// Shows the widget next to the mouse cursor, kept inside the monitor's work area.
pub fn show(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    let size = w.outer_size().unwrap_or(PhysicalSize::new(340, 480));
    let (width, height) = (size.width as f64, size.height as f64);

    if let Ok(cursor) = app.cursor_position() {
        if let Ok(Some(monitor)) = w.monitor_from_point(cursor.x, cursor.y) {
            let area = monitor.work_area();
            let scale = monitor.scale_factor();
            let margin = MARGIN * scale;
            let (left, top) = (area.position.x as f64, area.position.y as f64);
            let (right, bottom) = (left + area.size.width as f64, top + area.size.height as f64);

            let anchored_bottom = cursor.y > top + (bottom - top) / 2.0;
            let x =
                (cursor.x - width / 2.0).clamp(left + margin, (right - width - margin).max(left));
            let y = if anchored_bottom {
                bottom - height - margin
            } else {
                top + margin
            };
            *app.state::<WidgetState>().anchored_bottom.lock().unwrap() = anchored_bottom;
            let _ = w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        }
    }

    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
    let _ = w.emit("widget-shown", ());
}

/// Resizes the window to fit its content (height in logical px).
pub fn fit_height(app: &AppHandle, logical_height: f64) {
    let Some(w) = window(app) else { return };
    let scale = w.scale_factor().unwrap_or(1.0);
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) else {
        return;
    };
    let new_height = (logical_height * scale).round() as u32;
    if new_height == size.height {
        return;
    }
    let _ = w.set_size(PhysicalSize::new(size.width, new_height));
    if *app.state::<WidgetState>().anchored_bottom.lock().unwrap() {
        let y = pos.y + size.height as i32 - new_height as i32;
        let _ = w.set_position(PhysicalPosition::new(pos.x, y));
    }
}

/// Windows 11: ask DWM for rounded corners on our undecorated window.
#[cfg(windows)]
pub fn round_corners(w: &WebviewWindow) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    if let Ok(hwnd) = w.hwnd() {
        let pref = DWMWCP_ROUND;
        unsafe {
            let _ = DwmSetWindowAttribute(
                windows::Win32::Foundation::HWND(hwnd.0),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as _,
                std::mem::size_of_val(&pref) as u32,
            );
        }
    }
}

#[cfg(not(windows))]
pub fn round_corners(_w: &WebviewWindow) {}
