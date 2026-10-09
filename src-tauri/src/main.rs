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
}

fn default_output(devices: &[Device]) -> Option<String> {
    devices
        .iter()
        .find(|d| d.flow == Flow::Output && d.is_default)
        .map(|d| d.id.clone())
}

fn persist_mirrors(app: &AppHandle, state: &AppState) {
    let mirrors = state.mirrors.lock().unwrap().active_ids();
    settings::save(app, &settings::Settings { mirrors });
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

    let mirrored = state.mirrors.lock().unwrap().active_ids();
    for d in &mut devices {
        d.mirrored = mirrored.contains(&d.id);
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
fn resize_widget(app: AppHandle, height: f64) {
    widget::fit_height(&app, height);
}

#[tauri::command]
fn hide_widget(app: AppHandle) {
    widget::hide(&app);
}

#[tauri::command]
fn open_sound_settings(app: AppHandle) -> Result<(), String> {
    widget::hide(&app);
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

fn restore_mirrors(app: &AppHandle) {
    let saved = settings::load(app).mirrors;
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
        .tooltip("Audio Widget")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => widget::show(app),
            "settings" => {
                let _ = tauri_plugin_opener::open_url("ms-settings:sound", None::<&str>);
            }
            "quit" => {
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
            if !handle_args(app, &argv) {
                widget::show(app);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
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
        .manage(AppState::default())
        .manage(widget::WidgetState::default())
        .setup(|app| {
            let handle = app.handle();

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
            }
            restore_mirrors(handle);

            let args: Vec<String> = std::env::args().collect();
            if !handle_args(handle, &args) {
                widget::show(handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => widget::on_blur(window.app_handle()),
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
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
            resize_widget,
            hide_widget,
            open_sound_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Audio Widget");
}
