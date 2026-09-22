use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lumen_capture::synthetic::{Scene, SyntheticSource};
use lumen_capture::CaptureSource;
use lumen_core::{CaptureScheduler, ChangeDetector, ChangeReport, Frame, Rect, Responsiveness, SchedulerConfig};
use lumen_language::{identify, Language, Tracker};
use lumen_layout::{analyse, Alignment, Block, LayoutConfig, StrokeWeight};
use lumen_overlay::compositor::{Composition, Compositor};
use lumen_overlay::surface::{MemorySurface, OverlaySurface};
use lumen_overlay::{FontWeight, OverlayBlock, OverlayLayout, OverlayStyle, TextAlign};
use lumen_render::writing_mode_of;
use lumen_source::{merge, MergePolicy, ReadRequest, SourceError, TextRun, TextSource, TextTarget};
use lumen_stability::{Observation, StabilityConfig, StabilityTracker, StableBlock};
use lumen_translate::{StubTranslationEngine, TranslateItem, TranslationEngine, TranslationRequest};

use crate::report::{percentile, BlockRecord, FrameRecord, Report, Totals};
use crate::source::SceneTextSource;
use crate::Options;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("unknown scene: {0}")]
    UnknownScene(String),
    #[error("capture failed: {0}")]
    Capture(#[from] lumen_capture::CaptureError),
    #[error("a text source failed: {0}")]
    Source(#[from] lumen_source::SourceError),
    #[error("the overlay surface rejected a frame: {0}")]
    Surface(#[from] lumen_overlay::SurfaceError),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} could not be read as a PNG: {detail}")]
    Png { path: PathBuf, detail: String },
    #[error("the report could not be serialized: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// How the pipeline spends a pass.
#[derive(Debug, Clone, Copy)]
pub struct PassConfig {
    /// Reuse the previous pass on unchanged tiles: a static frame runs nothing beyond change
    /// detection, and a changed frame re-reads only the regions that changed.
    pub reuse_previous: bool,
    /// Pixels added around each changed region when re-reading. Zero reads exactly the changed
    /// tiles: change detection is tile-accurate, so no unchanged tile can hold new text. A real
    /// recognition engine that wants context pixels sets this where it wraps its adapter.
    pub read_margin: u32,
}

impl Default for PassConfig {
    fn default() -> Self {
        Self {
            reuse_previous: true,
            read_margin: 0,
        }
    }
}

/// Everything a [`PassRunner`] needs to start, in one place.
pub struct PassParams {
    pub width: u32,
    pub height: u32,
    pub tile: u32,
    pub style: OverlayStyle,
    pub speed: Responsiveness,
    pub config: PassConfig,
    pub source: Option<Box<dyn TextSource>>,
    pub engine: Box<dyn TranslationEngine>,
}

/// What a pass leaves behind for the next frame.
#[derive(Debug, Clone)]
struct PassState {
    /// The merged runs the last pass produced. Text that has stopped moving is still on screen,
    /// so it is kept instead of re-read.
    merged_runs: Vec<TextRun>,
    /// The last layout handed to the compositor, for the compose-skip decision.
    layout: Option<OverlayLayout>,
    /// The overlay blocks the last pass produced, so a skipped frame reports what the screen
    /// shows without recomputing anything.
    blocks: Vec<OverlayBlock>,
}

impl PassState {
    fn empty() -> Self {
        Self {
            merged_runs: Vec::new(),
            layout: None,
            blocks: Vec::new(),
        }
    }
}

/// What one frame cost and what the overlay shows after it, in a form the report, the soak run
/// and the chaos run all consume.
#[derive(Debug)]
pub struct PassOutcome {
    pub change: ChangeReport,
    /// The blocks the overlay shows after this frame.
    pub blocks: Vec<OverlayBlock>,
    pub language: Language,
    pub overlay_damage: Vec<Rect>,
    pub presented_pixels: u64,
    pub capture_rate_hz: f32,
    pub next_delay_ms: u64,
    pub detect_micros: u128,
    pub read_micros: u128,
    pub stage_micros: u128,
    pub compose_micros: u128,
    pub pass_micros: u128,
    /// True when the frame was static and nothing ran beyond change detection.
    pub skipped: bool,
    /// True when a changed frame recomposed nothing because the layout and the pixels under it
    /// were both unchanged.
    pub compose_skipped: bool,
    /// True when the target window was lost and the overlay removed itself.
    pub cleared: bool,
    /// True when a text source failed and the previous state was kept.
    pub read_error: bool,
}

/// Drives one window through the pipeline pass by pass.
///
/// The harness, the soak run and the chaos run all call this, so the code they measure is the
/// code the product runs, and the static-frame skip is available to all of them.
pub struct PassRunner {
    detector: ChangeDetector,
    scheduler: CaptureScheduler,
    compositor: Compositor,
    surface: MemorySurface,
    source: Option<Box<dyn TextSource>>,
    engine: Box<dyn TranslationEngine>,
    tracker: Tracker,
    stability: StabilityTracker,
    target: TextTarget,
    style: OverlayStyle,
    config: PassConfig,
    merge_policy: MergePolicy,
    layout_config: LayoutConfig,
    stability_config: StabilityConfig,
    state: Option<PassState>,
    /// Set by a fail-open clear: the next pass must re-read the whole frame, because the pass
    /// state that would have carried the unchanged text is gone.
    full_next_read: bool,
    last_presented: u64,
    last_composition: Option<Frame>,
    last_read_regions: Vec<Rect>,
    read_calls: usize,
}

impl PassRunner {
    pub fn new(params: PassParams) -> Self {
        let PassParams {
            width,
            height,
            tile,
            style,
            speed,
            config,
            source,
            engine,
        } = params;
        Self {
            detector: ChangeDetector::new(tile),
            scheduler: CaptureScheduler::new(SchedulerConfig {
                responsiveness: speed,
                ..SchedulerConfig::default()
            }),
            compositor: Compositor::new(),
            surface: MemorySurface::new(width, height),
            source,
            engine,
            tracker: Tracker::with_default_config(),
            stability: StabilityTracker::new(),
            target: TextTarget {
                id: 1,
                bounds: Rect::new(0, 0, width, height),
            },
            style,
            config,
            merge_policy: MergePolicy::default(),
            layout_config: LayoutConfig::default(),
            stability_config: StabilityConfig::default(),
            state: None,
            full_next_read: false,
            last_presented: 0,
            last_composition: None,
            last_read_regions: Vec::new(),
            read_calls: 0,
        }
    }

    /// The composition the overlay currently shows, when there is one. A skipped frame shows the
    /// previous one, which is what the harness writes to disk for it.
    pub fn last_composition(&self) -> Option<&Frame> {
        self.last_composition.as_ref()
    }

    pub fn surface(&self) -> &MemorySurface {
        &self.surface
    }

    /// The regions the last read was restricted to. An empty list means the whole frame.
    pub fn last_read_regions(&self) -> &[Rect] {
        &self.last_read_regions
    }

    /// How many times a text source has been asked to read.
    pub const fn read_calls(&self) -> usize {
        self.read_calls
    }

    /// The language the tracker has settled on for this window.
    pub fn final_language(&self) -> Language {
        self.tracker.language_of(self.target.id).unwrap_or(Language::Unknown)
    }

    /// Steps the text source to the next captured frame without reading. Called once per frame,
    /// so a source whose answer depends on the frame index stays in step with the capture no
    /// matter how many frames actually needed a read.
    pub fn advance_source(&mut self) {
        if let Some(source) = self.source.as_mut() {
            source.advance();
        }
    }

    /// Runs the pipeline once over `frame`.
    pub fn run_frame(&mut self, frame: &Frame) -> Result<PassOutcome, RunError> {
        let pass_started = Instant::now();
        let detect_started = Instant::now();
        let change = self.detector.accept(frame);
        let detect_micros = detect_started.elapsed().as_micros();
        let next_delay = self.scheduler.next_delay(&change);

        if self.config.reuse_previous && self.state.is_some() && !change.reset && change.is_static() {
            // Nothing moved: the overlay stands, the host is untouched, and the whole pass costs
            // the change detection itself. That is what "a static screen costs almost nothing"
            // means in code.
            let state = self.state.as_ref().expect("the skip condition checked for a previous pass");
            return Ok(PassOutcome {
                change: change.clone(),
                blocks: state.blocks.clone(),
                language: self.final_language(),
                overlay_damage: Vec::new(),
                presented_pixels: 0,
                capture_rate_hz: self.scheduler.current_hz(),
                next_delay_ms: next_delay.as_millis() as u64,
                detect_micros,
                read_micros: 0,
                stage_micros: 0,
                compose_micros: 0,
                pass_micros: pass_started.elapsed().as_micros(),
                skipped: true,
                compose_skipped: false,
                cleared: false,
                read_error: false,
            });
        }

        if self.source.is_some() {
            self.run_text_pass(frame, &change, pass_started, detect_micros, next_delay)
        } else {
            self.run_region_pass(frame, &change, pass_started, detect_micros, next_delay)
        }
    }

    fn run_text_pass(
        &mut self,
        frame: &Frame,
        change: &ChangeReport,
        pass_started: Instant,
        detect_micros: u128,
        next_delay: Duration,
    ) -> Result<PassOutcome, RunError> {
        let full_read = !self.config.reuse_previous || change.reset || self.full_next_read;
        self.full_next_read = false;
        let regions: Vec<Rect> = if full_read {
            Vec::new()
        } else {
            change
                .regions
                .iter()
                .map(|region| region.inflate(self.config.read_margin))
                .collect()
        };

        let read_started = Instant::now();
        self.last_read_regions = regions.clone();
        self.read_calls += 1;
        let read = {
            let source = self.source.as_mut().expect("run_text_pass is called for a text source");
            source.read(&ReadRequest {
                frame,
                target: &self.target,
                regions: &regions,
            })
        };
        let fresh = match read {
            Ok(runs) => runs,
            Err(SourceError::TargetLost) => {
                let cleared_started = Instant::now();
                let (overlay_damage, presented_pixels) = self.fail_open_clear(frame)?;
                return Ok(PassOutcome {
                    change: change.clone(),
                    blocks: Vec::new(),
                    language: self.final_language(),
                    overlay_damage,
                    presented_pixels,
                    capture_rate_hz: self.scheduler.current_hz(),
                    next_delay_ms: next_delay.as_millis() as u64,
                    detect_micros,
                    read_micros: read_started.elapsed().as_micros(),
                    stage_micros: 0,
                    compose_micros: cleared_started.elapsed().as_micros(),
                    pass_micros: pass_started.elapsed().as_micros(),
                    skipped: false,
                    compose_skipped: false,
                    cleared: true,
                    read_error: false,
                });
            }
            Err(_) => {
                // A transient failure keeps the previous reading: the overlay stays as it was,
                // the host application is untouched, and the next frame tries again.
                let state = self.state.clone().unwrap_or_else(PassState::empty);
                return Ok(PassOutcome {
                    change: change.clone(),
                    blocks: state.blocks,
                    language: self.final_language(),
                    overlay_damage: Vec::new(),
                    presented_pixels: 0,
                    capture_rate_hz: self.scheduler.current_hz(),
                    next_delay_ms: next_delay.as_millis() as u64,
                    detect_micros,
                    read_micros: read_started.elapsed().as_micros(),
                    stage_micros: 0,
                    compose_micros: 0,
                    pass_micros: pass_started.elapsed().as_micros(),
                    skipped: false,
                    compose_skipped: false,
                    cleared: false,
                    read_error: true,
                });
            }
        };
        let read_micros = read_started.elapsed().as_micros();

        // The unchanged part of the screen is still what the previous pass read, so it is kept
        // instead of re-read. That is the whole cost model of a static screen.
        let kept = if full_read {
            Vec::new()
        } else {
            self.state
                .as_ref()
                .map(|state| state.merged_runs.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|run| !change.regions.iter().any(|region| region.intersects(&run.bounds)))
                .collect()
        };

        let mut runs = fresh;
        runs.extend(kept);
        let merged = merge(runs, &self.merge_policy);

        self.tracker.observe(self.target.id, identify(&joined(&merged)));
        let language = self.final_language();

        let stage_started = Instant::now();
        let analysed = analyse(frame, merged.clone(), &self.layout_config);
        let observations = observations_of(&analysed);
        self.stability.observe(&observations, &self.stability_config);
        translate_pending(&mut *self.engine, &mut self.stability, language);
        let stable = self.stability.tracks().to_vec();
        let blocks = overlay_blocks_from_stable(&analysed, &stable);
        let stage_micros = stage_started.elapsed().as_micros();

        let layout = OverlayLayout::new(self.style).with_blocks(blocks.clone());

        // Re-composing when nothing the overlay shows changed would re-present identical
        // pixels, and on a layered window that is how flicker is born. Skip the compose when
        // the layout stands and no changed region touches what the overlay already drew.
        let layout_stands = self.state.as_ref().and_then(|state| state.layout.as_ref()) == Some(&layout);
        let source_touched_overlay = change.regions.iter().any(|region| {
            let painted = self.compositor.last_painted();
            painted.iter().any(|area| area.intersects(region))
        });
        let compose_skipped = self.config.reuse_previous && layout_stands && !source_touched_overlay;

        let (overlay_damage, presented_pixels, compose_micros) = if compose_skipped {
            (Vec::new(), 0, 0)
        } else {
            let compose_started = Instant::now();
            let composition = self.compositor.compose(frame, &layout);
            let pixels = self.present(&composition)?;
            (composition.damage, pixels, compose_started.elapsed().as_micros())
        };

        self.state = Some(PassState {
            merged_runs: merged,
            layout: Some(layout),
            blocks: blocks.clone(),
        });

        Ok(PassOutcome {
            change: change.clone(),
            blocks,
            language,
            overlay_damage,
            presented_pixels,
            capture_rate_hz: self.scheduler.current_hz(),
            next_delay_ms: next_delay.as_millis() as u64,
            detect_micros,
            read_micros,
            stage_micros,
            compose_micros,
            pass_micros: pass_started.elapsed().as_micros(),
            skipped: false,
            compose_skipped,
            cleared: false,
            read_error: false,
        })
    }

    /// The path for a plain PNG, which has no text source: one block stands in per changed
    /// region, which exercises erasure, damage tracking and presentation cost. A PNG is a single
    /// frame, so there is nothing to reuse.
    fn run_region_pass(
        &mut self,
        frame: &Frame,
        change: &ChangeReport,
        pass_started: Instant,
        detect_micros: u128,
        next_delay: Duration,
    ) -> Result<PassOutcome, RunError> {
        let stage_started = Instant::now();
        let blocks = region_blocks(&change.regions);
        let stage_micros = stage_started.elapsed().as_micros();

        let layout = OverlayLayout::new(self.style).with_blocks(blocks.clone());
        let compose_started = Instant::now();
        let composition = self.compositor.compose(frame, &layout);
        let presented_pixels = self.present(&composition)?;
        let compose_micros = compose_started.elapsed().as_micros();

        self.state = Some(PassState {
            merged_runs: Vec::new(),
            layout: Some(layout),
            blocks: blocks.clone(),
        });

        Ok(PassOutcome {
            change: change.clone(),
            blocks,
            language: Language::Unknown,
            overlay_damage: composition.damage,
            presented_pixels,
            capture_rate_hz: self.scheduler.current_hz(),
            next_delay_ms: next_delay.as_millis() as u64,
            detect_micros,
            read_micros: 0,
            stage_micros,
            compose_micros,
            pass_micros: pass_started.elapsed().as_micros(),
            skipped: false,
            compose_skipped: false,
            cleared: false,
            read_error: false,
        })
    }

    /// Every pipeline failure ends here: the overlay removes itself and the host application is
    /// left exactly as it was.
    fn fail_open_clear(&mut self, frame: &Frame) -> Result<(Vec<Rect>, u64), RunError> {
        // The pass state is dropped, not emptied: with no previous pass the static-frame
        // skip cannot trigger, so the next pass re-reads the whole frame even when the
        // pixels did not move (ADR 0007). An emptied state would report an empty overlay
        // for a screen that still shows text until something else changed.
        self.state = None;
        self.full_next_read = true;
        self.compositor.invalidate();
        let layout = OverlayLayout::new(self.style);
        let composition = self.compositor.compose(frame, &layout);
        let pixels = self.present(&composition)?;
        Ok((composition.damage, pixels))
    }

    fn present(&mut self, composition: &Composition) -> Result<u64, RunError> {
        self.surface.present(&composition.frame, &composition.damage)?;
        let total = self.surface.presented_pixels();
        let pixels = total.saturating_sub(self.last_presented);
        self.last_presented = total;
        self.last_composition = Some(composition.frame.clone());
        Ok(pixels)
    }
}

pub fn execute(options: &Options) -> Result<String, RunError> {
    let (source_name, frames, scene) = load_frames(options)?;
    let (width, height) = frames
        .first()
        .map(|frame| (frame.width(), frame.height()))
        .unwrap_or((options.width, options.height));

    fs::create_dir_all(&options.out_dir).map_err(|source| RunError::Io {
        path: options.out_dir.clone(),
        source,
    })?;

    let config = PassConfig {
        reuse_previous: options.reuse_previous,
        ..PassConfig::default()
    };
    let mut runner = match &scene {
        Some(scene) => {
            let source: Box<dyn TextSource> = Box::new(SceneTextSource::new(scene.clone()));
            let engine: Box<dyn TranslationEngine> = Box::new(StubTranslationEngine::new());
            PassRunner::new(PassParams {
                width,
                height,
                tile: options.tile,
                style: options.style,
                speed: options.speed,
                config,
                source: Some(source),
                engine,
            })
        }
        None => {
            let engine: Box<dyn TranslationEngine> = Box::new(StubTranslationEngine::new());
            PassRunner::new(PassParams {
                width,
                height,
                tile: options.tile,
                style: options.style,
                speed: options.speed,
                config,
                source: None,
                engine,
            })
        }
    };

    let mut records = Vec::with_capacity(frames.len());
    let mut static_frames = 0;

    for (index, frame) in frames.iter().enumerate() {
        let outcome = runner.run_frame(frame)?;
        if outcome.skipped {
            static_frames += 1;
        }
        runner.advance_source();

        let overlay_path = if options.write_images {
            match runner.last_composition() {
                Some(composition) => {
                    let path = options.out_dir.join(format!("overlay-{index:03}.png"));
                    write_png(&path, composition)?;
                    Some(path.display().to_string())
                }
                None => None,
            }
        } else {
            None
        };

        records.push(frame_record(index, &outcome, overlay_path));
    }

    let report = build_report(&source_name, width, height, options, &runner, records, static_frames);
    let report_path = options.out_dir.join("report.json");
    let text = serde_json::to_string_pretty(&report)?;
    fs::write(&report_path, text).map_err(|source| RunError::Io {
        path: report_path.clone(),
        source,
    })?;

    Ok(format!(
        "{}\nreport written to {}",
        report.summarize(),
        report_path.display()
    ))
}

fn frame_record(index: usize, outcome: &PassOutcome, overlay_path: Option<String>) -> FrameRecord {
    FrameRecord {
        index,
        changed_tiles: outcome.change.changed_tiles,
        total_tiles: outcome.change.total_tiles,
        changed_fraction: outcome.change.changed_fraction(),
        change_regions: outcome.change.regions.clone(),
        blocks: describe_blocks(&outcome.blocks),
        language: outcome.language,
        overlay_damage: outcome.overlay_damage.clone(),
        presented_pixels: outcome.presented_pixels,
        capture_rate_hz: outcome.capture_rate_hz,
        next_delay_ms: outcome.next_delay_ms,
        detect_micros: outcome.detect_micros,
        read_micros: outcome.read_micros,
        stage_micros: outcome.stage_micros,
        compose_micros: outcome.compose_micros,
        pass_micros: outcome.pass_micros,
        skipped: outcome.skipped,
        compose_skipped: outcome.compose_skipped,
        cleared: outcome.cleared,
        read_error: outcome.read_error,
        overlay_path,
    }
}

fn build_report(
    source_name: &str,
    width: u32,
    height: u32,
    options: &Options,
    runner: &PassRunner,
    records: Vec<FrameRecord>,
    static_frames: usize,
) -> Report {
    let full_surface_pixels = width as u64 * height as u64 * records.len() as u64;
    let presented_pixels: u64 = records.iter().map(|record| record.presented_pixels).sum();
    let detect: Vec<u128> = records.iter().map(|record| record.detect_micros).collect();
    let compose: Vec<u128> = records.iter().map(|record| record.compose_micros).collect();
    let pass: Vec<u128> = records.iter().map(|record| record.pass_micros).collect();
    let read: Vec<u128> = records.iter().map(|record| record.read_micros).collect();
    let stage: Vec<u128> = records.iter().map(|record| record.stage_micros).collect();
    let static_pass: Vec<u128> = records
        .iter()
        .filter(|record| record.skipped)
        .map(|record| record.pass_micros)
        .collect();
    let changed_pass: Vec<u128> = records
        .iter()
        .filter(|record| !record.skipped)
        .map(|record| record.pass_micros)
        .collect();

    let totals = Totals {
        frames: records.len(),
        static_frames,
        skipped_frames: records.iter().filter(|record| record.skipped).count(),
        cleared_frames: records.iter().filter(|record| record.cleared).count(),
        read_error_frames: records.iter().filter(|record| record.read_error).count(),
        presented_pixels,
        full_surface_pixels,
        presentation_savings: if full_surface_pixels == 0 {
            0.0
        } else {
            1.0 - presented_pixels as f32 / full_surface_pixels as f32
        },
        detect_micros_p50: percentile(detect.clone(), 0.5),
        detect_micros_max: detect.iter().copied().max().unwrap_or(0),
        compose_micros_p50: percentile(compose.clone(), 0.5),
        compose_micros_max: compose.iter().copied().max().unwrap_or(0),
        pass_micros_p50: percentile(pass.clone(), 0.5),
        pass_micros_p95: percentile(pass, 0.95),
        static_pass_micros_p95: percentile(static_pass, 0.95),
        changed_pass_micros_p95: percentile(changed_pass, 0.95),
        read_micros_p95: percentile(read, 0.95),
        stage_micros_p95: percentile(stage, 0.95),
    };

    Report {
        source: source_name.to_owned(),
        width,
        height,
        tile_size: options.tile,
        style: options.style,
        language: runner.final_language(),
        frames: records,
        totals,
    }
}

/// Everything a pass read, in one string, which is what language identification works on.
fn joined(runs: &[TextRun]) -> String {
    let parts: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
    parts.join(" ")
}

/// What stability sees: one observation per analysed block.
fn observations_of(blocks: &[Block]) -> Vec<Observation> {
    blocks
        .iter()
        .map(|block| Observation {
            rect: block.bounds,
            text: block.text(),
            confidence: block.confidence,
        })
        .collect()
}

/// Asks the engine for every stable block that still needs a translation and files the answer
/// against the track, so the next frame reuses it instead of calling again.
fn translate_pending(
    engine: &mut dyn TranslationEngine,
    stability: &mut StabilityTracker,
    source_language: Language,
) {
    let pending: Vec<String> = stability
        .tracks()
        .iter()
        .filter(|block| block.needs_translation())
        .map(|block| block.source_text.clone())
        .collect();
    if pending.is_empty() || source_language == Language::Unknown {
        return;
    }
    let items: Vec<TranslateItem> = pending
        .into_iter()
        .enumerate()
        .map(|(id, text)| TranslateItem { id, text, kind: None })
        .collect();
    let request = TranslationRequest {
        items,
        source_language,
        target_language: Language::Russian,
        context: None,
        app_id: None,
    };
    if let Ok(response) = engine.translate(&request) {
        for item in response.items {
            stability.provide_translation(&item.source, item.translated);
        }
    }
}

/// What the compositor draws: the stable reading, its typography and its translation when the
/// track has one. Geometry comes from the latest match, colours from the frame this pass.
fn overlay_blocks_from_stable(analysed: &[Block], stable: &[StableBlock]) -> Vec<OverlayBlock> {
    stable
        .iter()
        .map(|block| {
            let style = analysed
                .iter()
                .find(|candidate| candidate.bounds.iou(&block.rect) >= 0.3)
                .or_else(|| analysed.first());
            let (background, foreground, font_size, weight, align) = match style {
                Some(source) => (
                    source.background,
                    source.foreground,
                    source.font_size.max(10),
                    source.weight,
                    source.alignment,
                ),
                None => (
                    [24, 22, 20, 255],
                    [240, 238, 236, 255],
                    16,
                    StrokeWeight::Regular,
                    Alignment::Left,
                ),
            };
            let text = block.display_text().to_owned();
            let writing = writing_mode_of(&text, block.rect.width, block.rect.height);
            OverlayBlock::new(block.rect, text)
                .with_colors(background, foreground)
                .with_confidence(block.confidence)
                .with_font(
                    font_size,
                    match weight {
                        StrokeWeight::Regular => FontWeight::Regular,
                        StrokeWeight::Bold => FontWeight::Bold,
                    },
                    false,
                )
                .with_align(match align {
                    Alignment::Left => TextAlign::Left,
                    Alignment::Center => TextAlign::Center,
                    Alignment::Right => TextAlign::Right,
                })
                .with_writing(writing)
        })
        .collect()
}

fn describe_blocks(blocks: &[OverlayBlock]) -> Vec<BlockRecord> {
    blocks
        .iter()
        .map(|block| BlockRecord {
            rect: block.rect,
            text: block.text.clone(),
        })
        .collect()
}

/// Stands in for recognition on a plain PNG. No engine reads an image yet, so one block is drawn
/// per changed region, which is enough to exercise erasure, damage tracking and presentation cost.
/// A scene goes through the real path: text source, merge, layout analysis.
fn region_blocks(regions: &[Rect]) -> Vec<OverlayBlock> {
    regions
        .iter()
        .enumerate()
        .map(|(index, rect)| {
            OverlayBlock::new(*rect, format!("region {index}")).with_colors([24, 22, 20, 255], [240, 238, 236, 255])
        })
        .collect()
}

fn load_frames(options: &Options) -> Result<(String, Vec<Frame>, Option<Scene>), RunError> {
    if let Some(path) = &options.input {
        let frame = read_png(path)?;
        return Ok((path.display().to_string(), vec![frame], None));
    }

    let name = options.scene.as_deref().unwrap_or("menu");
    let scene = match name {
        "menu" => Scene::menu_appearing(options.width, options.height),
        other => return Err(RunError::UnknownScene(other.to_owned())),
    };

    let mut source = SyntheticSource::new(scene.clone());
    let mut frames = Vec::new();
    while let Some(frame) = source.next_frame()? {
        frames.push(frame);
    }
    Ok((format!("scene:{name}"), frames, Some(scene)))
}

fn read_png(path: &Path) -> Result<Frame, RunError> {
    let file = File::open(path).map_err(|source| RunError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().map_err(|error| RunError::Png {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|error| RunError::Png {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;

    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => {
            return Err(RunError::Png {
                path: path.to_path_buf(),
                detail: format!("{other:?} is not supported, use RGB or RGBA"),
            })
        }
    };

    let mut pixels = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    for chunk in buffer[..info.buffer_size()].chunks_exact(channels) {
        pixels.extend_from_slice(&[
            chunk[2],
            chunk[1],
            chunk[0],
            if channels == 4 { chunk[3] } else { 255 },
        ]);
    }

    Frame::packed(info.width, info.height, pixels).map_err(|error| RunError::Png {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

fn write_png(path: &Path, frame: &Frame) -> Result<(), RunError> {
    let file = File::create(path).map_err(|source| RunError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), frame.width(), frame.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder.write_header().map_err(|error| RunError::Png {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;

    let mut rgba = Vec::with_capacity(frame.width() as usize * frame.height() as usize * 4);
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let [b, g, r, a] = frame.pixel(x, y);
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }

    writer.write_image_data(&rgba).map_err(|error| RunError::Png {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_source::SourceKind;

    fn install_annotations() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                let msg = info.to_string().replace('\n', " | ");
                eprintln!("::error title=test-panic::{msg}");
            }));
        });
    }

    fn options(dir: &Path) -> Options {
        Options {
            scene: Some("menu".to_owned()),
            input: None,
            out_dir: dir.to_path_buf(),
            width: 640,
            height: 480,
            tile: 64,
            style: OverlayStyle::Seamless,
            speed: Responsiveness::Balanced,
            write_images: false,
            reuse_previous: true,
            soak_frames: None,
            chaos_frames: None,
            seed: 0,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn run_menu_scene(reuse_previous: bool) -> Report {
        let dir = temp_dir(if reuse_previous { "reuse" } else { "full" });
        let mut options = options(&dir);
        options.reuse_previous = reuse_previous;
        execute(&options).expect("the menu scene runs");
        let text = fs::read_to_string(dir.join("report.json")).expect("the report was written");
        fs::remove_dir_all(&dir).expect("the temp dir is removed");
        serde_json::from_str(&text).expect("the report parses")
    }

    #[test]
    fn a_scene_run_writes_a_report_for_every_frame() {
        install_annotations();
        let dir = temp_dir("report");
        execute(&options(&dir)).unwrap();
        let text = fs::read_to_string(dir.join("report.json")).unwrap();
        let report: Report = serde_json::from_str(&text).unwrap();
        assert_eq!(report.totals.frames, 8);
        assert!(report.totals.static_frames > 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_scene_run_reports_the_text_the_pipeline_read() {
        install_annotations();
        let dir = temp_dir("text");
        execute(&options(&dir)).unwrap();
        let text = fs::read_to_string(dir.join("report.json")).unwrap();
        let report: Report = serde_json::from_str(&text).unwrap();

        let last = report.frames.last().expect("a frame");
        let found: Vec<&str> = last.blocks.iter().map(|block| block.text.as_str()).collect();
        assert!(found.iter().any(|text| text.contains("Настройки")), "{found:?}");
        assert!(last.blocks.iter().all(|block| !block.text.is_empty()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_scene_run_identifies_the_language_of_its_own_text() {
        install_annotations();
        let dir = temp_dir("language");
        execute(&options(&dir)).unwrap();
        let text = fs::read_to_string(dir.join("report.json")).unwrap();
        let report: Report = serde_json::from_str(&text).unwrap();

        // The scene's first two frames are empty, so there is nothing to identify yet; from the
        // frame the menu appears the text is Russian and the tracker settles on it at once.
        assert_eq!(report.frames[0].language, Language::Unknown);
        assert_eq!(report.frames[2].language, Language::Russian);
        assert_eq!(report.language, Language::Russian);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn damage_tracking_avoids_most_of_the_surface() {
        install_annotations();
        let dir = temp_dir("savings");
        execute(&options(&dir)).unwrap();
        let text = fs::read_to_string(dir.join("report.json")).unwrap();
        let report: Report = serde_json::from_str(&text).unwrap();
        assert!(report.totals.presentation_savings > 0.5, "{:?}", report.totals);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unknown_scene_is_an_error() {
        install_annotations();
        let dir = temp_dir("unknown");
        let mut options = options(&dir);
        options.scene = Some("dungeon".to_owned());
        assert!(matches!(execute(&options), Err(RunError::UnknownScene(_))));
    }

    #[test]
    fn overlay_images_round_trip_through_png() {
        install_annotations();
        let dir = temp_dir("png");
        let mut options = options(&dir);
        options.write_images = true;
        execute(&options).unwrap();
        let written = read_png(&dir.join("overlay-002.png")).unwrap();
        assert_eq!((written.width(), written.height()), (640, 480));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_static_frame_runs_nothing_beyond_change_detection() {
        install_annotations();
        let report = run_menu_scene(true);

        // The menu scene is static between the menu and the tooltip, and while the tooltip
        // holds, so there must be skipped frames to check.
        let skipped: Vec<&FrameRecord> = report.frames.iter().filter(|frame| frame.skipped).collect();
        assert!(!skipped.is_empty(), "{:?}", report.totals);
        for frame in skipped {
            assert_eq!(frame.read_micros, 0, "frame {}", frame.index);
            assert_eq!(frame.stage_micros, 0, "frame {}", frame.index);
            assert_eq!(frame.compose_micros, 0, "frame {}", frame.index);
            assert_eq!(frame.presented_pixels, 0, "frame {}", frame.index);
            assert!(frame.overlay_damage.is_empty(), "frame {}", frame.index);
        }
    }

    fn runner_for_scene(scene: Scene, config: PassConfig) -> PassRunner {
        let source: Box<dyn TextSource> = Box::new(SceneTextSource::new(scene.clone()));
        let engine: Box<dyn TranslationEngine> = Box::new(StubTranslationEngine::new());
        PassRunner::new(PassParams {
            width: scene.width,
            height: scene.height,
            tile: 64,
            style: OverlayStyle::Seamless,
            speed: Responsiveness::Balanced,
            config,
            source: Some(source),
            engine,
        })
    }

    #[test]
    fn a_changed_frame_reads_only_the_changed_regions() {
        install_annotations();
        let scene = Scene::menu_appearing(640, 480);
        let mut capture = SyntheticSource::new(scene.clone());
        let mut runner = runner_for_scene(scene, PassConfig::default());

        // Frames 0 and 1 show only the background; the menu appears on frame 2.
        for _ in 0..3 {
            let frame = capture.next_frame().expect("a scene frame").expect("not the end");
            runner.run_frame(&frame).expect("the pass runs");
            runner.advance_source();
        }
        assert_eq!(runner.read_calls(), 2, "frame 0 is a reset, frame 2 is a change");
        // The re-read is restricted to the changed tiles, not the whole frame.
        let regions = runner.last_read_regions().to_vec();
        assert!(!regions.is_empty(), "a changed frame must read its changed regions");
        let whole_frame = Rect::new(0, 0, 640, 480);
        let read_area: u64 = regions.iter().map(|region| region.area()).sum();
        assert!(read_area < whole_frame.area() / 2, "menu is a third of the screen");
    }

    /// Drives the menu scene to the end and returns what each frame showed, what the overlay
    /// holds at the end, and how many reads happened.
    fn drive_menu_scene(reuse_previous: bool) -> (Vec<(Vec<BlockRecord>, Language)>, Vec<u8>, usize) {
        let scene = Scene::menu_appearing(640, 480);
        let mut capture = SyntheticSource::new(scene.clone());
        let config = PassConfig {
            reuse_previous,
            ..PassConfig::default()
        };
        let mut runner = runner_for_scene(scene, config);
        let mut per_frame = Vec::new();
        while let Some(frame) = capture.next_frame().expect("a scene frame") {
            let outcome = runner.run_frame(&frame).expect("the pass runs");
            runner.advance_source();
            per_frame.push((describe_blocks(&outcome.blocks), outcome.language));
        }
        let surface = runner.surface().frame().as_bytes().to_vec();
        (per_frame, surface, runner.read_calls())
    }

    #[test]
    fn reusing_the_previous_pass_matches_a_full_read() {
        install_annotations();
        let (reused, reused_surface, reused_reads) = drive_menu_scene(true);
        let (full, full_surface, full_reads) = drive_menu_scene(false);

        assert_eq!(reused.len(), full.len());
        for (index, ((reused_blocks, reused_language), (full_blocks, full_language))) in
            reused.iter().zip(full.iter()).enumerate()
        {
            assert_eq!(reused_blocks, full_blocks, "frame {index} blocks differ");
            assert_eq!(reused_language, full_language, "frame {index} language differs");
        }
        // The overlay ends up holding the same pixels either way: reuse is an optimisation,
        // not a different pipeline. It is also allowed to read less, but never more.
        assert_eq!(reused_surface, full_surface, "the overlay ends up different");
        assert!(
            reused_reads <= full_reads,
            "reuse read {reused_reads} passes, the full path read {full_reads}",
        );
    }

    #[test]
    fn the_portable_stages_stay_within_their_budgets() {
        install_annotations();
        let dir = temp_dir("budgets");
        let mut options = options(&dir);
        options.width = 1280;
        options.height = 720;
        execute(&options).unwrap();
        let text = fs::read_to_string(dir.join("report.json")).unwrap();
        let report: Report = serde_json::from_str(&text).unwrap();
        let totals = &report.totals;
        fs::remove_dir_all(&dir).unwrap();

        // A skipped frame runs only change detection, so it costs at most the detection of the
        // worst frame, on any machine. The absolute ceilings are wide on purpose: they budget
        // the portable stages on a build agent, not the reference machine, and they stop a
        // regression from landing silently.
        assert!(
            totals.skipped_frames > 0,
            "the menu scene must have static stretches: {totals:?}",
        );
        assert!(
            totals.static_pass_micros_p95 <= totals.detect_micros_max,
            "a skipped frame must cost at most the detection of the worst frame: {totals:?}",
        );
        assert!(totals.changed_pass_micros_p95 <= 500_000, "{totals:?}");
        assert!(totals.read_micros_p95 <= 200_000, "{totals:?}");
        assert!(totals.stage_micros_p95 <= 200_000, "{totals:?}");
    }

    struct FailingAfter {
        inner: SceneTextSource,
        fail_at: usize,
        reads: usize,
    }

    impl TextSource for FailingAfter {
        fn name(&self) -> &str {
            "failing-after"
        }

        fn kind(&self) -> SourceKind {
            SourceKind::Ocr
        }

        fn read(&mut self, request: &ReadRequest<'_>) -> Result<Vec<TextRun>, SourceError> {
            self.reads += 1;
            if self.reads == self.fail_at {
                return Err(SourceError::TargetLost);
            }
            self.inner.read(request)
        }

        fn advance(&mut self) {
            self.inner.advance()
        }
    }

    fn next_frame(capture: &mut SyntheticSource) -> Frame {
        capture.next_frame().expect("a scene frame").expect("not the end")
    }

    #[test]
    fn a_lost_window_clears_the_overlay_and_the_run_continues() {
        install_annotations();
        let scene = Scene::menu_appearing(640, 480);
        let mut capture = SyntheticSource::new(scene.clone());
        let source = FailingAfter {
            inner: SceneTextSource::new(scene.clone()),
            fail_at: 3,
            reads: 0,
        };
        let boxed: Box<dyn TextSource> = Box::new(source);
        let engine: Box<dyn TranslationEngine> = Box::new(StubTranslationEngine::new());
        let mut runner = PassRunner::new(PassParams {
            width: scene.width,
            height: scene.height,
            tile: 64,
            style: OverlayStyle::Seamless,
            speed: Responsiveness::Balanced,
            config: PassConfig::default(),
            source: Some(boxed),
            engine,
        });

        // Frame 0 is the reset (read 1), frame 2 brings the menu (read 2), and frame 4 is read
        // 3, which reports the window as gone.
        for index in 0..5 {
            let frame = next_frame(&mut capture);
            let outcome = runner.run_frame(&frame).expect("the pass runs");
            runner.advance_source();
            if index == 2 || index == 3 {
                assert!(
                    outcome.blocks.iter().any(|block| block.text.contains("Настройки")),
                    "frame {index}: {:?}",
                    outcome.blocks,
                );
            }
        }
        let frame = next_frame(&mut capture);
        let cleared = runner.run_frame(&frame).expect("the pass runs");
        runner.advance_source();
        assert!(cleared.cleared, "the third read reports the window as gone");
        assert!(cleared.blocks.is_empty());
        assert!(
            runner.surface().frame().as_bytes().iter().all(|byte| *byte == 0),
            "the overlay is removed",
        );

        // The window is back, and the overlay comes back with the whole menu, not only the
        // region that changed: a fail-open clear invalidates the pass state.
        let frame = next_frame(&mut capture);
        let outcome = runner.run_frame(&frame).expect("the pass runs");
        runner.advance_source();
        assert!(!outcome.cleared);
        assert!(
            outcome.blocks.iter().any(|block| block.text.contains("Настройки")),
            "{:?}",
            outcome.blocks,
        );
        assert!(
            outcome.blocks.iter().any(|block| block.text.contains("Удерживайте")),
            "{:?}",
            outcome.blocks,
        );
    }
}
