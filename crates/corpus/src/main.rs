//! The corpus harness: generate the scenes, score the gate, write it all down.
//!
//! The same library the tests run, as a command: scenes go to PNGs, ground truth to a manifest,
//! scores to a report, and the exit status is the gate verdict — zero when recognition is under
//! the plan's ceilings and identification is scored, non-zero when the gate fails. That exit
//! status is what a CI step can hang on; until such a step exists the workspace tests carry the
//! gate, and this binary produces the artifacts an owner can look at.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lumen_corpus::gate::{self, Thresholds};
use lumen_corpus::plan::CorpusPlan;
use lumen_corpus::report::{CorpusManifest, CorpusReport, GenerationSummary, SceneRecord};
use lumen_corpus::scene;

const DEFAULT_SEED: u64 = 0x5EED_2026;

const USAGE: &str = "\
lumen-corpus — generate and score the synthetic recognition corpus

Usage:
  lumen-corpus [options]

Options:
  --out-dir <path>      Where scenes, the manifest and the report are written (default: corpus-out)
  --width <pixels>      Scene width (default: 960)
  --height <pixels>     Scene height (default: 540)
  --limit <n>           Scenes per language/style/background cell (default: 1)
  --lines <n>           Lines per scene (default: 4)
  --seed <n>            Base seed every scene seed is drawn from (default: 1592598566)
  --no-images           Write only the manifest and the report
  -h, --help            Show this message

Exit status:
  0  the gate passed
  1  the gate failed, or the run itself failed
";

#[derive(Debug)]
pub struct Options {
    pub out_dir: PathBuf,
    pub width: u32,
    pub height: u32,
    pub limit: usize,
    pub lines: usize,
    pub seed: u64,
    pub write_images: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("corpus-out"),
            width: 960,
            height: 540,
            limit: 1,
            lines: 4,
            seed: DEFAULT_SEED,
            write_images: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Corpus(#[from] lumen_corpus::CorpusError),
    #[error("recognition scoring failed: {0}")]
    Ocr(#[from] lumen_ocr::OcrError),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} could not be written as a PNG: {detail}")]
    Png { path: PathBuf, detail: String },
    #[error("a document could not be serialized: {0}")]
    Serialize(#[from] serde_json::Error),
}

fn main() -> ExitCode {
    let options = match parse(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match execute(&options) {
        Ok(report) => {
            println!("{}", report.summarize());
            println!("report written to {}", options.out_dir.join("report.json").display());
            if report.gate_passed {
                ExitCode::SUCCESS
            } else {
                eprintln!("the recognition gate failed");
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("corpus run failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse(args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--out-dir" => options.out_dir = PathBuf::from(value()?),
            "--width" => options.width = parse_number(&value()?, "--width")?,
            "--height" => options.height = parse_number(&value()?, "--height")?,
            "--limit" => options.limit = parse_number(&value()?, "--limit")? as usize,
            "--lines" => options.lines = parse_number(&value()?, "--lines")? as usize,
            "--seed" => {
                let text = value()?;
                options.seed = match text.parse() {
                    Ok(number) => number,
                    Err(_) => return Err(format!("--seed expects a whole number, got {text:?}")),
                };
            }
            "--no-images" => options.write_images = false,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if options.width == 0 || options.height == 0 {
        return Err("width and height must be greater than zero".to_owned());
    }
    if options.limit == 0 || options.lines == 0 {
        return Err("limit and lines must be greater than zero".to_owned());
    }

    Ok(Some(options))
}

fn parse_number(value: &str, flag: &str) -> Result<u32, String> {
    match value.parse() {
        Ok(number) => Ok(number),
        Err(_) => Err(format!("{flag} expects a whole number, got {value}")),
    }
}

/// The whole run: plan, compose, write the ground truth, score both stages, write the report.
pub fn execute(options: &Options) -> Result<CorpusReport, RunError> {
    let plan = CorpusPlan {
        width: options.width,
        height: options.height,
        limit: options.limit,
        seed: options.seed,
        lines_per_scene: options.lines,
    };
    let specs = plan.scenes();

    io(|| fs::create_dir_all(&options.out_dir), &options.out_dir)?;
    let mut scenes = Vec::with_capacity(specs.len());
    for spec in &specs {
        scenes.push(scene::compose(spec)?);
    }

    let mut records = Vec::with_capacity(scenes.len());
    for (index, composed) in scenes.iter().enumerate() {
        let image = if options.write_images {
            let name = format!("scene-{index:03}.png");
            write_png(&options.out_dir.join(&name), &composed.frame)?;
            Some(name)
        } else {
            None
        };
        records.push(SceneRecord {
            name: composed.name.clone(),
            language: composed.language,
            style: composed.style,
            background: composed.background.kind(),
            lines: composed.lines.clone(),
            image,
        });
    }
    let manifest = CorpusManifest {
        width: plan.width,
        height: plan.height,
        seed: plan.seed,
        scenes: records,
    };
    write_json(&options.out_dir.join("corpus.json"), &manifest)?;

    let material = gate::prepare(scenes);
    let languages = material.languages.clone();
    let mut engine = material.perfect_engine();
    let recognition = gate::score_recognition(&material, &mut engine, &Thresholds::plan_defaults())?;
    let identification = gate::score_identification(&material.transcripts);

    let report = CorpusReport {
        generation: GenerationSummary::new(&plan, &languages),
        gate_passed: recognition.passed,
        recognition,
        identification,
    };
    write_json(&options.out_dir.join("report.json"), &report)?;
    Ok(report)
}

fn io<T>(action: impl FnOnce() -> std::io::Result<T>, path: &Path) -> Result<T, RunError> {
    match action() {
        Ok(value) => Ok(value),
        Err(source) => Err(RunError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write_json(path: &Path, document: &impl serde::Serialize) -> Result<(), RunError> {
    let text = serde_json::to_string_pretty(document)?;
    io(|| fs::write(path, text), path)?;
    Ok(())
}

/// Writes a frame as RGBA PNG; frames are BGRA, the capture pipeline's byte order.
fn write_png(path: &Path, frame: &lumen_core::Frame) -> Result<(), RunError> {
    let file = io(|| File::create(path), path)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), frame.width(), frame.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = match encoder.write_header() {
        Ok(w) => w,
        Err(error) => {
            return Err(RunError::Png {
                path: path.to_path_buf(),
                detail: error.to_string(),
            });
        }
    };

    let mut rgba = Vec::with_capacity(frame.width() as usize * frame.height() as usize * 4);
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let [b, g, r, a] = frame.pixel(x, y);
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }

    if let Err(error) = writer.write_image_data(&rgba) {
        return Err(RunError::Png {
            path: path.to_path_buf(),
            detail: error.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Option<Options>, String> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-corpus-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_describe_the_standard_plan() {
        let options = parse_args(&[]).unwrap().unwrap();
        assert_eq!(options.out_dir, PathBuf::from("corpus-out"));
        assert_eq!((options.width, options.height), (960, 540));
        assert_eq!(options.limit, 1);
        assert_eq!(options.lines, 4);
        assert_eq!(options.seed, DEFAULT_SEED);
        assert!(options.write_images);
    }

    #[test]
    fn help_stops_before_running() {
        assert!(parse_args(&["--help"]).unwrap().is_none());
        assert!(parse_args(&["-h"]).unwrap().is_none());
    }

    #[test]
    fn bad_arguments_are_rejected() {
        assert!(parse_args(&["--turbo"]).is_err());
        assert!(parse_args(&["--width"]).is_err());
        assert!(parse_args(&["--width", "0"]).is_err());
        assert!(parse_args(&["--limit", "0"]).is_err());
        assert!(parse_args(&["--seed", "soon"]).is_err());
    }

    #[test]
    fn every_knob_is_parsed() {
        let options = parse_args(&[
            "--out-dir",
            "target/corpus",
            "--width",
            "480",
            "--height",
            "270",
            "--limit",
            "2",
            "--lines",
            "3",
            "--seed",
            "42",
            "--no-images",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(options.out_dir, PathBuf::from("target/corpus"));
        assert_eq!((options.width, options.height), (480, 270));
        assert_eq!(options.limit, 2);
        assert_eq!(options.lines, 3);
        assert_eq!(options.seed, 42);
        assert!(!options.write_images);
    }

    #[test]
    fn a_small_run_writes_the_manifest_and_a_passing_report() {
        let dir = temp_dir("run");
        let options = Options {
            out_dir: dir.clone(),
            width: 240,
            height: 160,
            limit: 1,
            lines: 3,
            seed: DEFAULT_SEED,
            write_images: true,
        };
        let report = execute(&options).unwrap();

        assert!(report.gate_passed);
        let counted: usize = report.generation.languages.iter().map(|l| l.scenes).sum();
        assert_eq!(report.generation.scenes, counted);
        assert!(dir.join("corpus.json").exists());
        assert!(dir.join("report.json").exists());
        assert!(dir.join("scene-000.png").exists());

        let text = fs::read_to_string(dir.join("corpus.json")).unwrap();
        let manifest: CorpusManifest = serde_json::from_str(&text).unwrap();
        assert_eq!(manifest.scenes.len(), report.generation.scenes);
        assert!(manifest.scenes.iter().all(|scene| scene.image.is_some()));
        assert!(manifest.scenes.iter().all(|scene| !scene.lines.is_empty()));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_images_writes_only_documents() {
        let dir = temp_dir("documents");
        let options = Options {
            out_dir: dir.clone(),
            width: 200,
            height: 140,
            limit: 1,
            lines: 3,
            seed: 9,
            write_images: false,
        };
        let report = execute(&options).unwrap();
        assert!(report.gate_passed);
        assert!(!dir.join("scene-000.png").exists());
        let text = fs::read_to_string(dir.join("corpus.json")).unwrap();
        let manifest: CorpusManifest = serde_json::from_str(&text).unwrap();
        assert!(manifest.scenes.iter().all(|scene| scene.image.is_none()));
        fs::remove_dir_all(&dir).unwrap();
    }
}
