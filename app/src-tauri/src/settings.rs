//! The gears the owner turns while the loop keeps running.
//!
//! Every field is read by the live loop once per tick, so a change in the settings window
//! applies to the very next frame — no restart, no rebuild. The same values are persisted to
//! the platform settings file so the next launch starts where the owner left off.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use lumen_overlay::OverlayStyle;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveSettings {
    /// Reads below this confidence are skipped and journaled with their number.
    pub min_confidence: f32,
    /// Glyph height as a share of the detected line height.
    pub font_scale: f32,
    /// How many fresh lines one tick may send to translation; the rest wait a beat.
    pub max_lines_per_tick: usize,
    /// 0..=1, applied to everything the overlay draws.
    pub opacity: f32,
    /// How many earlier source/translation pairs feed the LLM backends for coherence.
    pub context_lines: usize,
    /// Seamless erases the original and draws in place; Plate covers; Subtitles collect below.
    pub overlay_style: OverlayStyle,
    /// "local", "google", "deepl" or "openai". Local always stays the fallback when a
    /// network backend errors, so the overlay never goes silent.
    pub translator: String,
    pub deepl_key: String,
    pub openai_key: String,
    pub openai_model: String,
}

impl Default for LiveSettings {
    fn default() -> Self {
        Self {
            min_confidence: 0.75,
            font_scale: 0.72,
            max_lines_per_tick: 8,
            opacity: 1.0,
            context_lines: 6,
            overlay_style: OverlayStyle::Seamless,
            translator: "google".to_string(),
            deepl_key: String::new(),
            openai_key: String::new(),
            openai_model: "gpt-4o-mini".to_string(),
        }
    }
}

pub type LiveSettingsHandle = Arc<RwLock<LiveSettings>>;

pub fn path() -> PathBuf {
    let root = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    root.join("DynoTranslate").join("settings.json")
}

/// Loads the owner's saved gears, or the recommended defaults on a first run.
pub fn handle() -> LiveSettingsHandle {
    let settings = std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<LiveSettings>(&text).ok())
        .unwrap_or_default();
    Arc::new(RwLock::new(settings))
}

fn clamp_all(settings: &mut LiveSettings) {
    settings.min_confidence = settings.min_confidence.clamp(0.0, 0.95);
    settings.font_scale = settings.font_scale.clamp(0.4, 1.2);
    settings.max_lines_per_tick = settings.max_lines_per_tick.clamp(1, 24);
    settings.opacity = settings.opacity.clamp(0.3, 1.0);
    settings.context_lines = settings.context_lines.clamp(0, 20);
    if !matches!(settings.translator.as_str(), "local" | "google" | "deepl" | "openai") {
        settings.translator = "google".to_string();
    }
    if settings.openai_model.trim().is_empty() {
        settings.openai_model = "gpt-4o-mini".to_string();
    }
}

#[tauri::command]
pub fn get_live_settings(state: tauri::State<'_, LiveSettingsHandle>) -> LiveSettings {
    state.read().expect("live settings").clone()
}

#[tauri::command]
pub fn set_live_settings(state: tauri::State<'_, LiveSettingsHandle>, settings: LiveSettings) -> LiveSettings {
    let mut next = settings;
    clamp_all(&mut next);
    *state.write().expect("live settings") = next.clone();
    if let Some(dir) = path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&next) {
        let _ = std::fs::write(path(), text);
    }
    next
}
