use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lumen_capture::synthetic::{Scene, SyntheticSource};
use lumen_capture::CaptureSource;
use lumen_core::{CaptureScheduler, ChangeDetector, Frame, Rect, SchedulerConfig};
use lumen_language::{identify, Language, Tracker};
use lumen_layout::{analyse, Alignment, Block, LayoutConfig, StrokeWeight};
use lumen_overlay::compositor::Compositor;
use lumen_overlay::surface::{MemorySurface, OverlaySurface};
use lumen_overlay::{FontWeight, OverlayBlock, OverlayLayout, TextAlign};
use lumen_render::writing_mode_of;
use lumen_source::{merge, MergePolicy, ReadRequest, TextRun, TextSource, TextTarget};
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

    let mut detector = ChangeDetector::new(options.tile);
    let mut scheduler = CaptureScheduler::new(SchedulerConfig {
        responsiveness: options.speed,
        ..SchedulerConfig::default()
    });
    let mut compositor = Compositor::new();
    let mut surface = MemorySurface::new(width, height);

    let mut text_source = scene.map(SceneTextSource::new);
    let target = TextTarget {
        id: 1,
        bounds: Rect::new(0, 0, width, height),
    };
    let merge_policy = MergePolicy::default();
    let layout_config = LayoutConfig::default();
    let mut tracker = Tracker::with_default_config();
    let stability_config = StabilityConfig::default();
    let mut stability = StabilityTracker::new();
    let mut engine = StubTranslationEngine::new();

    let mut records = Vec::with_capacity(frames.len());
    let mut static_frames = 0;
    let mut presented_before = 0u64;

    for (index, frame) in frames.iter().enumerate() {
        let started = Instant::now();
        let change = detector.accept(frame);
        let detect_micros = started.elapsed().as_micros();

        if change.is_static() {
            static_frames += 1;
        }

        let (blocks, language) = match &mut text_source {
            Some(source) => {
                // The whole frame is read rather than only the regions that changed: text that
                // stopped moving is still on screen and still needs its translation. Skipping
                // work on unchanged tiles needs the previous pass to be reusable, which stays
                // open after M4.
                let request = ReadRequest {
                    frame,
                    target: &target,
                    regions: &[],
                };
                let runs = merge(source.read(&request)?, &merge_policy);
                tracker.observe(target.id, identify(&joined(&runs)));
                let language = tracker.language_of(target.id).unwrap_or(Language::Unknown);
                let analysed = analyse(frame, runs, &layout_config);
                let observations = observations_of(&analysed);
                stability.observe(&observations, &stability_config);
                translate_pending(&mut engine, &mut stability, language);
                let stable = stability.tracks().to_vec();
                (overlay_blocks_from_stable(&analysed, &stable), language)
            }
            None => (region_blocks(&change.regions), Language::Unknown),
        };
        let reported = describe_blocks(&blocks);
        let layout = OverlayLayout::new(options.style).with_blocks(blocks);

        let started = Instant::now();
        let composition = compositor.compose(frame, &layout);
        let compose_micros = started.elapsed().as_micros();

        surface.present(&composition.frame, &composition.damage)?;
        let presented_pixels = surface.presented_pixels() - presented_before;
        presented_before = surface.presented_pixels();

        let delay = scheduler.next_delay(&change);

        let overlay_path = if options.write_images {
            let path = options.out_dir.join(format!("overlay-{index:03}.png"));
            write_png(&path, &composition.frame)?;
            Some(path.display().to_string())
        } else {
            None
        };

        records.push(FrameRecord {
            index,
            changed_tiles: change.changed_tiles,
            total_tiles: change.total_tiles,
            changed_fraction: change.changed_fraction(),
            change_regions: change.regions,
            blocks: reported,
            language,
            overlay_damage: composition.damage,
            presented_pixels,
            capture_rate_hz: scheduler.current_hz(),
            next_delay_ms: delay.as_millis() as u64,
            detect_micros,
            compose_micros,
            overlay_path,
        });
    }

    let full_surface_pixels = width as u64 * height as u64 * records.len() as u64;
    let presented_pixels: u64 = records.iter().map(|record| record.presented_pixels).sum();
    let detect: Vec<u128> = records.iter().map(|record| record.detect_micros).collect();
    let compose: Vec<u128> = records.iter().map(|record| record.compose_micros).collect();

    let report = Report {
        source: source_name,
        width,
        height,
        tile_size: options.tile,
        style: options.style,
        language: tracker.language_of(target.id).unwrap_or(Language::Unknown),
        totals: Totals {
            frames: records.len(),
            static_frames,
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
        },
        frames: records,
    };

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
fn translate_pending(engine: &mut StubTranslationEngine, stability: &mut StabilityTracker, source_language: Language) {
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
        pixels.extend_from_slice(&[chunk[2], chunk[1], chunk[0], if channels == 4 { chunk[3] } else { 255 }]);
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
    use lumen_overlay::OverlayStyle;

    fn options(dir: &Path) -> Options {
        Options {
            scene: Some("menu".to_owned()),
            input: None,
            out_dir: dir.to_path_buf(),
            width: 640,
            height: 480,
            tile: 64,
            style: OverlayStyle::Seamless,
            speed: lumen_core::Responsiveness::Balanced,
            write_images: false,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_scene_run_writes_a_report_for_every_frame() {
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
        let dir = temp_dir("savings");
        execute(&options(&dir)).unwrap();
        let text = fs::read_to_string(dir.join("report.json")).unwrap();
        let report: Report = serde_json::from_str(&text).unwrap();
        assert!(report.totals.presentation_savings > 0.5, "{:?}", report.totals);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unknown_scene_is_an_error() {
        let dir = temp_dir("unknown");
        let mut options = options(&dir);
        options.scene = Some("dungeon".to_owned());
        assert!(matches!(execute(&options), Err(RunError::UnknownScene(_))));
    }

    #[test]
    fn overlay_images_round_trip_through_png() {
        let dir = temp_dir("png");
        let mut options = options(&dir);
        options.write_images = true;
        execute(&options).unwrap();
        let written = read_png(&dir.join("overlay-002.png")).unwrap();
        assert_eq!((written.width(), written.height()), (640, 480));
        fs::remove_dir_all(&dir).unwrap();
    }
}
