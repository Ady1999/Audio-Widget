//! Showing, hiding and sizing the HUD window.

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

pub fn show(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.hide();
    }
}

pub fn toggle(app: &AppHandle) {
    match window(app) {
        Some(w) if w.is_visible().unwrap_or(false) => hide(app),
        _ => show(app),
    }
}

/// Moves the window to a saved position, unless that spot is no longer on any monitor.
pub fn restore_position(w: &WebviewWindow, (x, y): (i32, i32)) {
    let on_screen = w.available_monitors().unwrap_or_default().iter().any(|m| {
        let (p, s) = (m.position(), m.size());
        x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
    });
    if on_screen {
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
}

/// Resizes the window to fit its content (height in logical px).
pub fn fit_height(app: &AppHandle, logical_height: f64) {
    let Some(w) = window(app) else { return };
    let (Ok(scale), Ok(size)) = (w.scale_factor(), w.outer_size()) else {
        return;
    };
    let height = (logical_height * scale).round() as u32;
    if height != size.height {
        let _ = w.set_size(PhysicalSize::new(size.width, height));
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
