//! The soak run: many frames, continuous change, and a flat memory profile.
//!
//! The plan's soak targets are "memory growth under 5 percent, flat". A deterministic scene
//! with random block lifetimes keeps the pipeline busy for the whole run, the counting allocator
//! in [`crate::alloc`] measures what the run leaks, and the report goes to stdout as JSON so the
//! integration test can assert the plan's thresholds on any machine.

use lumen_capture::synthetic::{Scene, SceneBlock, SyntheticSource};
use lumen_capture::CaptureSource;
use lumen_core::Rect;
use lumen_source::TextSource;
use lumen_translate::StubTranslationEngine;
use lumen_translate::TranslationEngine;
use serde::Serialize;

use crate::alloc;
use crate::alloc::AllocSnapshot;
use crate::report::percentile;
use crate::rng::SplitMix64;
use crate::run::{PassConfig, PassParams, PassRunner, RunError};
use crate::source::SceneTextSource;
use crate::Options;

/// Interface phrases in the languages the tracker has to keep steady, plus the numeric and
/// key-hint forms that must never be translated.
const PHRASES: &[&str] = &[
    "Начать игру",
    "Продолжить",
    "Настройки",
    "Выход",
    "Загрузить игру",
    "New Game",
    "Continue",
    "Options",
    "Inventory",
    "Quests",
    "Press [E] to interact",
    "87 / 100",
    "Уровень 42",
    "Press [F] for the map",
];

/// A deterministic scene: `frames` frames worth of blocks with random lifetimes, so a different
/// region of the screen changes on most frames and the pass reuse is exercised constantly.
pub(crate) fn random_scene(width: u32, height: u32, frames: usize, rng: &mut SplitMix64) -> Scene {
    // The block count scales with the run length, so a longer run stays busy: each block
    // appears once and usually disappears again, and the chaos fault plan is sized against
    // the reads those changes cause. A fixed count would leave a long run mostly static.
    let block_count = 6 + frames / 16;
    let mut blocks = Vec::with_capacity(block_count);
    for _ in 0..block_count {
        let block_width = 120 + rng.below(160) as u32;
        let block_height = 24 + rng.below(16) as u32;
        let x = 16 + rng.below(width.saturating_sub(block_width + 16).max(1) as u64) as i32;
        let y = 16 + rng.below(height.saturating_sub(block_height + 16).max(1) as u64) as i32;
        let text = PHRASES[rng.below(PHRASES.len() as u64) as usize].to_owned();
        let appears_at = rng.below((frames as u64) * 7 / 10 + 1) as usize;
        let mut block = SceneBlock::new(Rect::new(x, y, block_width, block_height), [214, 210, 204, 255], text)
            .from_frame(appears_at);
        if rng.below(10) <= 5 {
            let span = frames as u64 - appears_at as u64;
            if span > 4 {
                block = block.until_frame(appears_at + 3 + rng.below(span - 3) as usize);
            }
        }
        blocks.push(block);
    }
    Scene {
        width,
        height,
        background: [28, 24, 20, 255],
        blocks,
        frames,
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct SoakReport {
    pub frames: usize,
    pub static_frames: usize,
    pub changed_frames: usize,
    pub pass_micros_p50: u128,
    pub pass_micros_p95: u128,
    pub memory: MemoryReport,
}

#[derive(Debug, Serialize)]
pub(crate) struct MemoryReport {
    pub start_live_bytes: usize,
    pub mid_live_bytes: usize,
    pub end_live_bytes: usize,
    pub peak_live_bytes: usize,
    /// Live bytes at the end relative to the midpoint, in percent. The plan's ceiling is 5.
    pub growth_percent: f64,
    pub allocations: u64,
    pub deallocations: u64,
}

pub fn execute(frames: u32, options: &Options) -> Result<String, RunError> {
    let frames = frames as usize;
    let mut rng = SplitMix64::new(options.seed);
    let scene = random_scene(options.width, options.height, frames, &mut rng);

    let source: Box<dyn TextSource> = Box::new(SceneTextSource::new(scene.clone()));
    let engine: Box<dyn TranslationEngine> = Box::new(StubTranslationEngine::new());
    let mut runner = PassRunner::new(PassParams {
        width: options.width,
        height: options.height,
        tile: options.tile,
        style: options.style,
        speed: options.speed,
        config: PassConfig::default(),
        source: Some(source),
        engine,
    });

    let mut capture = SyntheticSource::new(scene);
    let start = alloc::snapshot();
    let mid_frame = frames / 2;
    let stride = frames.div_ceil(4096).max(1);
    let mut pass_samples: Vec<u128> = Vec::new();
    let mut static_frames = 0;
    let mut mid: Option<AllocSnapshot> = None;

    for index in 0..frames {
        let Some(frame) = capture.next_frame()? else {
            break;
        };
        let outcome = runner.run_frame(&frame)?;
        runner.advance_source();
        if outcome.skipped {
            static_frames += 1;
        }
        if index % stride == 0 {
            pass_samples.push(outcome.pass_micros);
        }
        if index == mid_frame {
            mid = Some(alloc::snapshot());
        }
    }

    let end = alloc::snapshot();
    let mid = mid.unwrap_or(start);
    let growth_percent = if mid.live_bytes == 0 {
        0.0
    } else {
        (end.live_bytes as f64 - mid.live_bytes as f64) / mid.live_bytes as f64 * 100.0
    };

    let report = SoakReport {
        frames,
        static_frames,
        changed_frames: frames - static_frames,
        pass_micros_p50: percentile(pass_samples.clone(), 0.5),
        pass_micros_p95: percentile(pass_samples, 0.95),
        memory: MemoryReport {
            start_live_bytes: start.live_bytes,
            mid_live_bytes: mid.live_bytes,
            end_live_bytes: end.live_bytes,
            peak_live_bytes: end.peak_bytes.max(start.peak_bytes),
            growth_percent,
            allocations: end.allocations - start.allocations,
            deallocations: end.deallocations - start.deallocations,
        },
    };

    serde_json::to_string(&report).map_err(RunError::Serialize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_plan_is_deterministic_for_a_seed() {
        let mut a = SplitMix64::new(7);
        let mut b = SplitMix64::new(7);
        let first = random_scene(640, 480, 300, &mut a);
        let second = random_scene(640, 480, 300, &mut b);
        assert_eq!(first.blocks.len(), second.blocks.len());
        for index in 0..300 {
            assert_eq!(first.labels_at(index), second.labels_at(index), "frame {index} differs");
        }
    }

    #[test]
    fn the_scene_blocks_stay_inside_the_frame() {
        for seed in 0..8 {
            let mut rng = SplitMix64::new(seed);
            let scene = random_scene(640, 480, 400, &mut rng);
            for block in &scene.blocks {
                assert!(block.rect.x >= 0);
                assert!(block.rect.y >= 0);
                assert!(block.rect.right() <= 640);
                assert!(block.rect.bottom() <= 480);
                assert!(!block.text.is_empty());
            }
        }
    }
}
