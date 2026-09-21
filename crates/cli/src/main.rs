//! Headless pipeline harness.
//!
//! Runs a scene (or a PNG) through change detection, scheduling and overlay composition, then
//! writes the composited overlay and a machine-readable report. Fidelity work and benchmarks are
//! driven through this binary so they produce the same numbers on a workstation and on a build
//! agent.

mod report;
mod run;
mod source;

use std::path::PathBuf;
use std::process::ExitCode;

use lumen_core::Responsiveness;

const USAGE: &str = "\
lumen-pipeline — run a frame sequence through the Lumen pipeline without a display

Usage:
  lumen-pipeline --scene menu [options]
  lumen-pipeline --input <frame.png> [options]

Options:
  --scene <name>        Built-in scene to run. Currently: menu
  --input <path>        Run a single PNG frame instead of a scene
  --out-dir <path>      Where the overlay PNGs and the report are written (default: pipeline-out)
  --width <pixels>      Scene width (default: 1280)
  --height <pixels>     Scene height (default: 720)
  --tile <pixels>       Change-detection tile size (default: 64)
  --style <name>        Overlay style: seamless, plate, subtitles (default: seamless)
  --speed <name>        Responsiveness: fast, balanced, accurate (default: balanced)
  --no-images           Write only the report
  -h, --help            Show this message
";

#[derive(Debug)]
pub struct Options {
    pub scene: Option<String>,
    pub input: Option<PathBuf>,
    pub out_dir: PathBuf,
    pub width: u32,
    pub height: u32,
    pub tile: u32,
    pub style: lumen_overlay::OverlayStyle,
    pub speed: Responsiveness,
    pub write_images: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            scene: None,
            input: None,
            out_dir: PathBuf::from("pipeline-out"),
            width: 1280,
            height: 720,
            tile: 64,
            style: lumen_overlay::OverlayStyle::Seamless,
            speed: Responsiveness::Balanced,
            write_images: true,
        }
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("LUMEN_LOG"))
        .with_target(false)
        .init();

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

    match run::execute(&options) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("pipeline failed: {error}");
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
            "--scene" => options.scene = Some(value()?),
            "--input" => options.input = Some(PathBuf::from(value()?)),
            "--out-dir" => options.out_dir = PathBuf::from(value()?),
            "--width" => options.width = parse_number(&value()?, "--width")?,
            "--height" => options.height = parse_number(&value()?, "--height")?,
            "--tile" => options.tile = parse_number(&value()?, "--tile")?,
            "--style" => {
                options.style = match value()?.as_str() {
                    "seamless" => lumen_overlay::OverlayStyle::Seamless,
                    "plate" => lumen_overlay::OverlayStyle::Plate,
                    "subtitles" => lumen_overlay::OverlayStyle::Subtitles,
                    other => return Err(format!("unknown overlay style: {other}")),
                }
            }
            "--speed" => {
                options.speed = match value()?.as_str() {
                    "fast" => Responsiveness::Fast,
                    "balanced" => Responsiveness::Balanced,
                    "accurate" => Responsiveness::Accurate,
                    other => return Err(format!("unknown speed: {other}")),
                }
            }
            "--no-images" => options.write_images = false,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if options.scene.is_none() && options.input.is_none() {
        options.scene = Some("menu".to_owned());
    }
    if options.width == 0 || options.height == 0 {
        return Err("width and height must be greater than zero".to_owned());
    }

    Ok(Some(options))
}

fn parse_number(value: &str, flag: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} expects a whole number, got {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Option<Options>, String> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn defaults_to_the_menu_scene() {
        let options = parse_args(&[]).unwrap().unwrap();
        assert_eq!(options.scene.as_deref(), Some("menu"));
        assert_eq!((options.width, options.height), (1280, 720));
    }

    #[test]
    fn help_stops_before_running() {
        assert!(parse_args(&["--help"]).unwrap().is_none());
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        assert!(parse_args(&["--turbo"]).is_err());
    }

    #[test]
    fn a_flag_without_its_value_is_rejected() {
        assert!(parse_args(&["--width"]).is_err());
    }

    #[test]
    fn styles_and_speeds_are_parsed() {
        let options = parse_args(&["--style", "plate", "--speed", "fast"]).unwrap().unwrap();
        assert_eq!(options.style, lumen_overlay::OverlayStyle::Plate);
        assert_eq!(options.speed, Responsiveness::Fast);
        assert!(parse_args(&["--style", "neon"]).is_err());
    }

    #[test]
    fn zero_dimensions_are_rejected() {
        assert!(parse_args(&["--width", "0"]).is_err());
    }
}
