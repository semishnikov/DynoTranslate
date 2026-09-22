use lumen_core::Rect;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameRecord {
    pub index: usize,
    pub changed_tiles: usize,
    pub total_tiles: usize,
    pub changed_fraction: f32,
    pub change_regions: Vec<Rect>,
    /// The text the pipeline produced for this frame, so a report says what was translated and
    /// where, not only how fast.
    pub blocks: Vec<BlockRecord>,
    /// The language the tracker had settled on by this frame.
    pub language: lumen_language::Language,
    pub overlay_damage: Vec<Rect>,
    pub presented_pixels: u64,
    pub capture_rate_hz: f32,
    pub next_delay_ms: u64,
    pub detect_micros: u128,
    pub read_micros: u128,
    pub stage_micros: u128,
    pub compose_micros: u128,
    /// The whole pass, wall time from frame in to present out.
    pub pass_micros: u128,
    /// True when the frame was static and nothing ran beyond change detection.
    pub skipped: bool,
    /// True when a changed frame recomposed nothing because the overlay content was unchanged.
    pub compose_skipped: bool,
    /// True when the target window was lost and the overlay removed itself.
    pub cleared: bool,
    /// True when a text source failed and the previous state was kept.
    pub read_error: bool,
    pub overlay_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockRecord {
    pub rect: Rect,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Totals {
    pub frames: usize,
    pub static_frames: usize,
    /// Static frames that ran nothing beyond change detection.
    pub skipped_frames: usize,
    pub cleared_frames: usize,
    pub read_error_frames: usize,
    pub presented_pixels: u64,
    pub full_surface_pixels: u64,
    /// Share of pixels avoided by presenting damage instead of whole frames.
    pub presentation_savings: f32,
    pub detect_micros_p50: u128,
    pub detect_micros_max: u128,
    pub compose_micros_p50: u128,
    pub compose_micros_max: u128,
    pub pass_micros_p50: u128,
    pub pass_micros_p95: u128,
    /// The cost of a skipped frame: change detection only.
    pub static_pass_micros_p95: u128,
    pub changed_pass_micros_p95: u128,
    pub read_micros_p95: u128,
    pub stage_micros_p95: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub tile_size: u32,
    pub style: lumen_overlay::OverlayStyle,
    /// The language the run settled on, which is what translation would be working from.
    pub language: lumen_language::Language,
    pub frames: Vec<FrameRecord>,
    pub totals: Totals,
}

impl Report {
    pub fn summarize(&self) -> String {
        let totals = &self.totals;
        format!(
            "{} · {}x{} · {} frames ({} static, {} skipped, {} cleared) · source language {}\n\
             detect  p50 {} µs, max {} µs\n\
             compose p50 {} µs, max {} µs\n\
             pass    p50 {} µs, p95 {} µs (static p95 {} µs, changed p95 {} µs)\n\
             presented {} of {} pixels ({:.1}% avoided)",
            self.source,
            self.width,
            self.height,
            totals.frames,
            totals.static_frames,
            totals.skipped_frames,
            totals.cleared_frames,
            self.language.name(),
            totals.detect_micros_p50,
            totals.detect_micros_max,
            totals.compose_micros_p50,
            totals.compose_micros_max,
            totals.pass_micros_p50,
            totals.pass_micros_p95,
            totals.static_pass_micros_p95,
            totals.changed_pass_micros_p95,
            totals.presented_pixels,
            totals.full_surface_pixels,
            totals.presentation_savings * 100.0,
        )
    }
}

pub fn percentile(mut values: Vec<u128>, percent: f32) -> u128 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let index = ((values.len() as f32 - 1.0) * percent).round() as usize;
    values[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_of_nothing_is_zero() {
        assert_eq!(percentile(Vec::new(), 0.5), 0);
    }

    #[test]
    fn percentile_picks_the_expected_sample() {
        let values = vec![10, 50, 20, 40, 30];
        assert_eq!(percentile(values.clone(), 0.5), 30);
        assert_eq!(percentile(values, 1.0), 50);
    }
}
