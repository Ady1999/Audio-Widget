#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod settings;
mod widget;

use std::sync::Mutex;

use audio::{Device, Flow, MirrorManager};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const URL_SCHEME: &str = "audiowidget://";
const SHORTCUT: &str = "ctrl+alt+a";

#[derive(Default)]
struct AppState {
    mirrors: Mutex<MirrorManager>,
    /// Last default output we saw, so mirrors can follow external default changes.
    last_default_output: Mutex<Option<String>>,
    settings: Mutex<settings::Settings>,
}

fn default_output(devices: &[Device]) -> Option<String> {
    devices
        .iter()
        .find(|d| d.flow == Flow::Output && d.is_default)
        .map(|d| d.id.clone())
}

fn persist_mirrors(app: &AppHandle, state: &AppState) {
    let (mirrors, delays) = {
        let mut m = state.mirrors.lock().unwrap();
        (m.active_ids(), m.delays.clone())
    };
    let mut s = state.settings.lock().unwrap();
    s.mirrors = mirrors;
    s.delays = delays;
    settings::save(app, &s);
}

#[tauri::command]
async fn get_devices(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<Device>, String> {
    let mut devices = audio::list_devices()?;

    // The default output was changed outside the widget (e.g. Windows settings):
    // re-attach mirrors to the new default.
    let current = default_output(&devices);
    let changed = {
        let mut last = state.last_default_output.lock().unwrap();
        let changed = last.is_some() && *last != current;
        *last = current.clone();
        changed
    };
    if changed {
        state
            .mirrors
            .lock()
            .unwrap()
            .restart_all(current.as_deref());
        persist_mirrors(&app, &state);
    }

    let mut mirrors = state.mirrors.lock().unwrap();
    let mirrored = mirrors.active_ids();
    for d in &mut devices {
        d.mirrored = mirrored.contains(&d.id);
        d.delay_ms = mirrors.delays.get(&d.id).copied().unwrap_or(0);
    }
    Ok(devices)
}

#[tauri::command]
async fn set_default_device(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    flow: Flow,
) -> Result<(), String> {
    audio::set_default(&id)?;
    if flow == Flow::Output {
        *state.last_default_output.lock().unwrap() = Some(id.clone());
        state.mirrors.lock().unwrap().restart_all(Some(&id));
        persist_mirrors(&app, &state);
    }
    Ok(())
}

#[tauri::command]
async fn set_volume(id: String, volume: f32) -> Result<(), String> {
    audio::set_volume(&id, volume)
}

#[tauri::command]
async fn set_mute(id: String, muted: bool) -> Result<(), String> {
    audio::set_mute(&id, muted)
}

#[tauri::command]
async fn set_mirror(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let result = state.mirrors.lock().unwrap().set(&id, enabled);
    persist_mirrors(&app, &state);
    result
}

#[tauri::command]
async fn set_delay(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    ms: u32,
) -> Result<(), String> {
    state.mirrors.lock().unwrap().set_delay(&id, ms);
    persist_mirrors(&app, &state);
    Ok(())
}

#[tauri::command]
fn resize_widget(app: AppHandle, height: f64) {
    widget::fit_height(&app, height);
}

#[tauri::command]
fn hide_widget(app: AppHandle) {
    widget::hide(&app);
}

#[tauri::command]
fn is_pinned(state: State<'_, AppState>) -> bool {
    state.settings.lock().unwrap().pinned
}

#[tauri::command]
fn set_pinned(app: AppHandle, state: State<'_, AppState>, pinned: bool) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.set_always_on_top(pinned).map_err(|e| e.to_string())?;
    }
    let mut s = state.settings.lock().unwrap();
    s.pinned = pinned;
    settings::save(&app, &s);
    Ok(())
}

/// Remembers where the user dragged the HUD.
fn save_position(app: &AppHandle) {
    let Some(Ok(pos)) = app.get_webview_window("main").map(|w| w.outer_position()) else {
        return;
    };
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap();
    if s.position != Some((pos.x, pos.y)) {
        s.position = Some((pos.x, pos.y));
        settings::save(app, &s);
    }
}

#[tauri::command]
fn open_sound_settings() -> Result<(), String> {
    tauri_plugin_opener::open_url("ms-settings:sound", None::<&str>).map_err(|e| e.to_string())
}

/// Handles command line / deep link arguments, from the first launch or a second instance.
/// Returns true if an argument was recognised.
fn handle_args(app: &AppHandle, args: &[String]) -> bool {
    for arg in args.iter().skip(1) {
        let action = arg
            .strip_prefix(URL_SCHEME)
            .map(|a| a.trim_end_matches('/'))
            .or_else(|| arg.strip_prefix("--"));
        match action {
            Some("toggle") => widget::toggle(app),
            Some("show") => widget::show(app),
            Some("hide") => widget::hide(app),
            Some("hidden") => {}
            _ => continue,
        }
        return true;
    }
    false
}

/// True when the launch came from a different exe than the one running, e.g. a newly
/// installed version while an old portable copy is still in the tray.
fn is_other_copy(launched: Option<&String>) -> bool {
    let canonical = |p: std::path::PathBuf| std::fs::canonicalize(p).ok();
    let launched = launched.and_then(|p| canonical(p.into()));
    let running = std::env::current_exe().ok().and_then(canonical);
    launched.is_some() && launched != running
}

/// Quits this instance and starts the other exe once we're gone (it would otherwise
/// just hand its launch back to us, since only one instance may run).
fn hand_over(app: &AppHandle, exe: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // ping waits ~1 s so our single-instance lock is released before the new copy starts.
        let _ = std::process::Command::new("cmd")
            .raw_arg(format!(
                "/C ping -n 2 127.0.0.1 >nul & start \"\" \"{exe}\""
            ))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = exe;
    app.state::<AppState>().mirrors.lock().unwrap().stop_all();
    app.exit(0);
}

fn restore_mirrors(app: &AppHandle) {
    let saved = app
        .state::<AppState>()
        .settings
        .lock()
        .unwrap()
        .mirrors
        .clone();
    if saved.is_empty() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let devices = audio::list_devices().unwrap_or_default();
        let default = default_output(&devices);
        let state = app.state::<AppState>();
        *state.last_default_output.lock().unwrap() = default.clone();
        let mut mirrors = state.mirrors.lock().unwrap();
        for id in saved {
            let available = devices.iter().any(|d| d.id == id && d.flow == Flow::Output);
            if available && default.as_deref() != Some(id.as_str()) {
                let _ = mirrors.set(&id, true);
            }
        }
    });
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Audio Widget", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Sound settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(format!("Audio Widget {}", app.package_info().version))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => widget::show(app),
            "settings" => {
                let _ = tauri_plugin_opener::open_url("ms-settings:sound", None::<&str>);
            }
            "quit" => {
                save_position(app);
                app.state::<AppState>().mirrors.lock().unwrap().stop_all();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                widget::toggle(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if is_other_copy(argv.first()) {
                hand_over(app, &argv[0]);
                return;
            }
            if !handle_args(app, &argv) {
                widget::show(app);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        widget::toggle(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle();
            let saved = settings::load(handle);

            #[cfg(windows)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                // Registers audiowidget:// for the current user so Seelen UI (or a
                // shortcut / Stream Deck / AutoHotkey) can open the widget.
                if let Err(e) = app.deep_link().register_all() {
                    eprintln!("failed to register {URL_SCHEME} scheme: {e}");
                }
            }

            if let Err(e) = app.global_shortcut().register(SHORTCUT) {
                eprintln!("failed to register shortcut {SHORTCUT}: {e}");
            }

            setup_tray(handle)?;
            if let Some(w) = app.get_webview_window("main") {
                widget::round_corners(&w);
                widget::attach_to_desktop(&w);
                let _ = w.set_always_on_top(saved.pinned);
                if let Some(pos) = saved.position {
                    widget::restore_position(&w, pos);
                }
            }
            let mut mirrors = MirrorManager::default();
            mirrors.delays = saved.delays.clone();
            app.manage(AppState {
                mirrors: Mutex::new(mirrors),
                settings: Mutex::new(saved),
                ..Default::default()
            });
            restore_mirrors(handle);

            let args: Vec<String> = std::env::args().collect();
            if !handle_args(handle, &args) {
                widget::show(handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Saving on blur instead of on every Moved event avoids disk writes mid-drag.
            WindowEvent::Focused(false) => save_position(window.app_handle()),
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                save_position(window.app_handle());
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_devices,
            set_default_device,
            set_volume,
            set_mute,
            set_mirror,
            set_delay,
            resize_widget,
            hide_widget,
            is_pinned,
            set_pinned,
            open_sound_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Audio Widget");
}
