//! Tauri host for Lumen.
//!
//! The React surface talks only through the commands below, so the same UI runs against a
//! mocked store in the browser and against this shell on the desktop. Settings the shell
//! needs at startup (hotkeys, tray click behaviour) arrive as JSON and are stored in the
//! platform settings file the owner wires in M6.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

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

/// One translated block returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslatedBlockDto {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub source_text: String,
    pub display_text: String,
    pub confidence: f32,
    pub is_new: bool,
}

/// One OCR observation submitted from the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationDto {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub confidence: f32,
}

/// Pipeline statistics for the frontend dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatsDto {
    pub frames: u64,
    pub blocks: u64,
    pub engine_calls: u64,
    pub items_translated: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub engine_time_ms: u64,
    pub last_frame_ms: u64,
}

/// The shared pipeline state.
struct PipelineState {
    pipeline: lumen_translate::LivePipeline,
    engine: lumen_translate::GoogleFreeEngine,
}

impl PipelineState {
    fn new() -> Self {
        Self {
            pipeline: lumen_translate::LivePipeline::new(lumen_translate::LiveConfig::default()),
            engine: lumen_translate::GoogleFreeEngine::new(),
        }
    }
}

#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

#[tauri::command]
fn get_shell_settings(state: tauri::State<'_, Mutex<ShellSettings>>) -> ShellSettings {
    state.lock().expect("shell settings").clone()
}

#[tauri::command]
fn set_shell_settings(
    state: tauri::State<'_, Mutex<ShellSettings>>,
    settings: ShellSettings,
) -> ShellSettings {
    let mut current = state.lock().expect("shell settings");
    current.hotkeys_enabled = settings.hotkeys_enabled;
    current.tray_quick_toggle = settings.tray_quick_toggle;
    current.tray_minimise = settings.tray_minimise;
    current.region = settings.region;
    current.clone()
}

/// Process a frame of observations and return translated blocks.
#[tauri::command]
fn translate_frame(
    state: tauri::State<'_, Mutex<PipelineState>>,
    observations: Vec<ObservationDto>,
) -> Vec<TranslatedBlockDto> {
    let mut state = state.lock().expect("pipeline state");

    let obs: Vec<lumen_stability::Observation> = observations
        .iter()
        .map(|o| lumen_stability::Observation {
            rect: lumen_core::Rect::new(o.x, o.y, o.width, o.height),
            text: o.text.clone(),
            confidence: o.confidence,
        })
        .collect();

    let blocks = state.pipeline.process_frame(&obs, &mut state.engine);

    blocks
        .iter()
        .map(|b| TranslatedBlockDto {
            x: b.rect.x,
            y: b.rect.y,
            width: b.rect.width,
            height: b.rect.height,
            source_text: b.source_text.clone(),
            display_text: b.display_text.clone(),
            confidence: b.confidence,
            is_new: b.is_new,
        })
        .collect()
}

/// Get current pipeline statistics.
#[tauri::command]
fn get_pipeline_stats(state: tauri::State<'_, Mutex<PipelineState>>) -> PipelineStatsDto {
    let state = state.lock().expect("pipeline state");
    let stats = state.pipeline.stats();
    PipelineStatsDto {
        frames: stats.frames,
        blocks: stats.blocks,
        engine_calls: stats.engine_calls,
        items_translated: stats.items_translated,
        cache_hits: stats.cache_hits,
        cache_misses: stats.cache_misses,
        engine_time_ms: stats.engine_time_ms,
        last_frame_ms: stats.last_frame_ms,
    }
}

/// Clear the translation cache.
#[tauri::command]
fn clear_cache(state: tauri::State<'_, Mutex<PipelineState>>) {
    let mut state = state.lock().expect("pipeline state");
    state.pipeline.clear_cache();
    state.pipeline.reset_tracking();
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(Mutex::new(ShellSettings::default()))
        .manage(Mutex::new(PipelineState::new()))
        .invoke_handler(tauri::generate_handler![
            ping,
            get_shell_settings,
            set_shell_settings,
            translate_frame,
            get_pipeline_stats,
            clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lumen shell");
}
