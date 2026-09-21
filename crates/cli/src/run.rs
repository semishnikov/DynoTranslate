use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lumen_capture::synthetic::{Scene, SyntheticSource};
use lumen_capture::CaptureSource;
use lumen_core::{CaptureScheduler, ChangeDetector, Frame, Rect, SchedulerConfig};
use lumen_ocr::layout::{analyze, Font, Line};
use lumen_ocr::source::{merge, OcrSource, SourceError, TextSource, TextSpan};
use lumen_ocr::stub::StubEngine;
use lumen_ocr::Recognition;
use lumen_overlay::compositor::Compositor;
use lumen_overlay::surface::{MemorySurface, OverlaySurface};
use lumen_overlay::{OverlayBlock, OverlayLayout};

use crate::report::{percentile, FrameRecord, Report, Totals};
use crate::Options;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("unknown scene: {0}")]
    UnknownScene(String),
    #[error("capture failed: {0}")]
    Capture(#[from] lumen_capture::CaptureError),
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
    #[error("the text sources failed: {0}")]
    Text(#[from] SourceError),
    #[error("the report could not be serialized: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn execute(options: &Options) -> Result<String, RunError> {
    let (source_name, frames) = load_frames(options)?;
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
    let mut text_source = OcrSource::new(StubEngine::new(recognition_script(options)));

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

        // The harness runs the same text stages the product does — source, merge, layout — with the
        // scripted engine standing in for recognition and no UIA source attached to the merge.
        let spans = merge(text_source.snapshot(frame)?, Vec::new());
        let blocks = overlay_blocks(frame, &spans);
        let texts = blocks.iter().map(|block| block.text.clone()).collect();

        let layout = OverlayLayout::new(options.style).with_blocks(blocks);

        let started = Instant::now();
        let composition = compositor.compose(width, height, &layout);
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
            overlay_damage: composition.damage,
            presented_pixels,
            capture_rate_hz: scheduler.current_hz(),
            next_delay_ms: delay.as_millis() as u64,
            detect_micros,
            compose_micros,
            texts,
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

/// What the deterministic engine reads off each frame of the menu scene. `--input` runs attach no
/// scene and therefore no transcript, and their reports show no text.
fn recognition_script(options: &Options) -> Vec<Vec<Recognition>> {
    if options.input.is_some() {
        return Vec::new();
    }
    (0..MENU_FRAMES).map(menu_answer).collect()
}

/// The number of frames `Scene::menu_appearing` runs for.
const MENU_FRAMES: usize = 8;

/// The text one frame of the menu scene is showing: its three items from the frame the rows appear
/// on, and the tooltip across the two frames it flickers for. Bounds sit over the ink bars the
/// scene paints, the way recognition bounds would sit over glyphs.
fn menu_answer(frame: usize) -> Vec<Recognition> {
    let line = |text: &str, bounds: Rect| Recognition {
        text: text.to_owned(),
        bounds,
        confidence: 0.9,
    };
    if frame < 2 {
        return Vec::new();
    }
    let mut answer = vec![
        line("New game", Rect::new(76, 67, 160, 26)),
        line("Continue", Rect::new(76, 115, 200, 26)),
        line("Settings", Rect::new(76, 163, 130, 26)),
    ];
    if frame == 5 || frame == 6 {
        answer.push(line("Autosaves every minute", Rect::new(532, 324, 200, 28)));
    }
    answer
}

/// Turns spans into the blocks the compositor presents: lines take their plate colour from the
/// frame, and the layout analysis groups and classifies them from there.
fn overlay_blocks(frame: &Frame, spans: &[TextSpan]) -> Vec<OverlayBlock> {
    let lines: Vec<Line> = spans
        .iter()
        .map(|span| Line::from_span(frame, span, Font::estimated(span.bounds.height)))
        .collect();
    analyze(frame, lines)
        .into_iter()
        .map(|block| OverlayBlock::new(block.bounds, block.text()).with_colors(block.background, block.text_color))
        .collect()
}

fn load_frames(options: &Options) -> Result<(String, Vec<Frame>), RunError> {
    if let Some(path) = &options.input {
        let frame = read_png(path)?;
        return Ok((path.display().to_string(), vec![frame]));
    }

    let name = options.scene.as_deref().unwrap_or("menu");
    let scene = match name {
        "menu" => Scene::menu_appearing(options.width, options.height),
        other => return Err(RunError::UnknownScene(other.to_owned())),
    };

    let mut source = SyntheticSource::new(scene);
    let mut frames = Vec::new();
    while let Some(frame) = source.next_frame()? {
        frames.push(frame);
    }
    Ok((format!("scene:{name}"), frames))
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

    #[test]
    fn the_script_matches_the_scene_it_reads() {
        assert_eq!(Scene::menu_appearing(640, 480).frames, MENU_FRAMES);
        assert!(menu_answer(0).is_empty());
        assert_eq!(menu_answer(2).len(), 3);
        assert_eq!(menu_answer(5).len(), 4);
    }

    #[test]
    fn recognised_text_becomes_the_overlay_blocks() {
        let frame = Scene::menu_appearing(640, 480).render(4).unwrap();
        let mut source = OcrSource::new(StubEngine::new(vec![menu_answer(4)]));
        let spans = merge(source.snapshot(&frame).unwrap(), Vec::new());
        let blocks = overlay_blocks(&frame, &spans);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "New game");
        assert_eq!(blocks[0].background, [214, 210, 204, 255]);
        assert_eq!(blocks[0].foreground, [34, 28, 22, 255]);
    }

    #[test]
    fn the_report_records_the_recognised_text() {
        let dir = temp_dir("texts");
        execute(&options(&dir)).unwrap();
        let text = fs::read_to_string(dir.join("report.json")).unwrap();
        let report: Report = serde_json::from_str(&text).unwrap();
        assert!(report.frames[0].texts.is_empty());
        assert_eq!(report.frames[2].texts, vec!["New game", "Continue", "Settings"]);
        assert_eq!(report.frames[5].texts.len(), 4);
        fs::remove_dir_all(&dir).unwrap();
    }
}
