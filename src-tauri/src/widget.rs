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
    let (Ok(scale), Ok(size)) = (w.scale_factor(), w.inner_size()) else {
        return;
    };
    let height = (logical_height * scale).round() as u32;
    if height == size.height {
        return;
    }
    let _ = w.set_size(PhysicalSize::new(size.width, height));
    // Growing must not push the bottom (or the drag bar) off the screen.
    if let (Ok(Some(m)), Ok(pos), Ok(outer)) =
        (w.current_monitor(), w.outer_position(), w.outer_size())
    {
        let area = m.work_area();
        let bottom = area.position.y + area.size.height as i32;
        let y = pos.y.min(bottom - outer.height as i32).max(area.position.y);
        if y != pos.y {
            let _ = w.set_position(PhysicalPosition::new(pos.x, y));
        }
    }
}

/// Makes the desktop own the window, so "Show desktop" (Win+D) and minimize-all leave
/// it visible, like a desktop gadget. Pinning on top still works on top of this.
#[cfg(windows)]
pub fn attach_to_desktop(w: &WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowLongPtrW, GWLP_HWNDPARENT,
    };
    use windows_core::w;
    if let (Ok(hwnd), Ok(progman)) = (w.hwnd(), unsafe { FindWindowW(w!("Progman"), None) }) {
        unsafe {
            SetWindowLongPtrW(
                windows::Win32::Foundation::HWND(hwnd.0),
                GWLP_HWNDPARENT,
                progman.0 as isize,
            )
        };
    }
}

#[cfg(not(windows))]
pub fn attach_to_desktop(_w: &WebviewWindow) {}

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
