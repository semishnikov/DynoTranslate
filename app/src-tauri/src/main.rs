//! Tauri host for Lumen.
//!
//! The React surface talks only through the commands below, so the same UI runs against a
//! mocked store in the browser and against this shell on the desktop. Settings the shell
//! needs at startup (hotkeys, tray click behaviour) arrive as JSON and are stored in the
//! platform settings file the owner wires in M6.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backends;
mod live;
mod model;
mod readtext;
mod settings;

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// Mirror of the React `RegionRect` (percent of the window).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RegionRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSettings {
    pub hotkeys_enabled: bool,
    pub tray_quick_toggle: bool,
    pub tray_minimise: bool,
    pub region: RegionRect,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            hotkeys_enabled: true,
            tray_quick_toggle: true,
            tray_minimise: true,
            region: RegionRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
        }
    }
}

#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

#[tauri::command]
fn get_shell_settings(state: tauri::State<'_, std::sync::Mutex<ShellSettings>>) -> ShellSettings {
    state.lock().expect("shell settings").clone()
}

#[tauri::command]
fn set_shell_settings(
    state: tauri::State<'_, std::sync::Mutex<ShellSettings>>,
    settings: ShellSettings,
) -> ShellSettings {
    let mut current = state.lock().expect("shell settings");
    current.hotkeys_enabled = settings.hotkeys_enabled;
    current.tray_quick_toggle = settings.tray_quick_toggle;
    current.tray_minimise = settings.tray_minimise;
    current.region = settings.region;
    current.clone()
}

#[tauri::command]
fn live_status(state: tauri::State<'_, live::Control>) -> live::LiveStatus {
    state.status()
}

#[tauri::command]
fn live_preview(state: tauri::State<'_, live::Control>) -> String {
    state.preview()
}

#[tauri::command]
fn set_paused(paused: bool, state: tauri::State<'_, live::Control>, app: tauri::AppHandle) {
    live::set_paused(&state, &app, paused);
}

#[tauri::command]
fn list_windows(state: tauri::State<'_, live::Control>) -> Vec<live::ListedWindow> {
    live::list_windows(&state)
}

#[tauri::command]
fn choose_window(id: String, state: tauri::State<'_, live::Control>, app: tauri::AppHandle) {
    live::choose_window(&state, &app, &id);
}

fn main() {
    // The repository is private, so the updater authenticates its release downloads with
    // a read-only token baked in at build time (release.yml sets UPDATER_PAT from a
    // secret). Builds without one — pull requests from forks, local checks — simply ship
    // an updater that cannot reach the private releases; see the UI error state.
    let updater = tauri_plugin_updater::Builder::new();
    let updater = match option_env!("UPDATER_PAT") {
        Some(pat) => updater
            .header("Authorization", format!("Bearer {pat}"))
            .expect("updater auth header"),
        None => updater,
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(updater.build())
        .plugin(tauri_plugin_process::init())
        .manage(std::sync::Mutex::new(ShellSettings::default()))
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unmaximize();
                let _ = window.unminimize();
                let _ = window.set_size(tauri::LogicalSize::new(680.0, 760.0));
                let _ = window.center();
                let _ = window.show();
                let _ = window.set_focus();
            }
            let bundled = app.path().resource_dir().ok();
            let app_handle = app.handle().clone();
            let control = live::control();
            app.manage(control.clone());
            let live_settings = settings::handle();
            app.manage(live_settings.clone());
            std::thread::spawn(move || live::run(app_handle, control, bundled, live_settings));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            get_shell_settings,
            set_shell_settings,
            live_status,
            live_preview,
            set_paused,
            list_windows,
            choose_window,
            settings::get_live_settings,
            settings::set_live_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lumen shell");
}
