//! Headless pipeline harness.
//!
//! Runs a scene (or a PNG) through change detection, scheduling and overlay composition, then
//! writes the composited overlay and a machine-readable report. Fidelity work and benchmarks are
//! driven through this binary so they produce the same numbers on a workstation and on a build
//! agent. The soak and chaos modes run the same pipeline under a long continuous workload and
//! under scripted faults, and report memory growth and fail-open behaviour as JSON.

mod alloc;
mod chaos;
mod report;
mod rng;
mod run;
mod soak;
mod source;

use std::path::PathBuf;
use std::process::ExitCode;

use lumen_core::Responsiveness;

const USAGE: &str = "\
lumen-pipeline — run a frame sequence through the Lumen pipeline without a display

Usage:
  lumen-pipeline --scene menu [options]
  lumen-pipeline --input <frame.png> [options]
  lumen-pipeline --soak <frames> [options]
  lumen-pipeline --chaos <frames> [options]

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
  --no-reuse            Read the whole frame on every pass, for comparison with the default
  --soak <frames>       Run the soak harness and print a JSON memory report
  --chaos <frames>      Run the chaos harness under scripted faults and print a JSON report
  --seed <number>       Seed for the soak and chaos scene plans (default 0)
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
    pub reuse_previous: bool,
    pub soak_frames: Option<u32>,
    pub chaos_frames: Option<u32>,
    pub seed: u64,
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
            reuse_previous: true,
            soak_frames: None,
            chaos_frames: None,
            seed: 0,
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

    if let Some(frames) = options.soak_frames {
        match soak::execute(frames, &options) {
            Ok(report) => {
                println!("{report}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("soak failed: {error}");
                ExitCode::FAILURE
            }
        }
    } else if let Some(frames) = options.chaos_frames {
        match chaos::execute(frames, &options) {
            Ok(report) => {
                println!("{report}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("chaos failed: {error}");
                ExitCode::FAILURE
            }
        }
    } else {
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
            "--no-reuse" => options.reuse_previous = false,
            "--soak" => options.soak_frames = Some(parse_number(&value()?, "--soak")?),
            "--chaos" => options.chaos_frames = Some(parse_number(&value()?, "--chaos")?),
            "--seed" => options.seed = parse_seed(&value()?, "--seed")?,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let no_run_selected = options.soak_frames.is_none() && options.chaos_frames.is_none();
    if options.scene.is_none() && options.input.is_none() && no_run_selected {
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

fn parse_seed(value: &str, flag: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} expects a whole number, got {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn parse_args(args: &[&str]) -> Result<Option<Options>, String> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn defaults_to_the_menu_scene() {
        install_annotations();
        let options = parse_args(&[]).unwrap().unwrap();
        assert_eq!(options.scene.as_deref(), Some("menu"));
        assert_eq!((options.width, options.height), (1280, 720));
        assert!(options.reuse_previous);
    }

    #[test]
    fn help_stops_before_running() {
        install_annotations();
        assert!(parse_args(&["--help"]).unwrap().is_none());
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        install_annotations();
        assert!(parse_args(&["--turbo"]).is_err());
    }

    #[test]
    fn a_flag_without_its_value_is_rejected() {
        install_annotations();
        assert!(parse_args(&["--width"]).is_err());
    }

    #[test]
    fn styles_and_speeds_are_parsed() {
        install_annotations();
        let options = parse_args(&["--style", "plate", "--speed", "fast"]).unwrap().unwrap();
        assert_eq!(options.style, lumen_overlay::OverlayStyle::Plate);
        assert_eq!(options.speed, Responsiveness::Fast);
        assert!(parse_args(&["--style", "neon"]).is_err());
    }

    #[test]
    fn zero_dimensions_are_rejected() {
        install_annotations();
        assert!(parse_args(&["--width", "0"]).is_err());
    }

    #[test]
    fn the_soak_and_chaos_flags_carry_a_frame_count_and_a_seed() {
        install_annotations();
        let options = parse_args(&["--soak", "600", "--seed", "7"]).unwrap().unwrap();
        assert_eq!(options.soak_frames, Some(600));
        assert_eq!(options.chaos_frames, None);
        assert_eq!(options.seed, 7);
        assert!(
            options.scene.is_none(),
            "a soak run does not fall back to the menu scene",
        );

        let options = parse_args(&["--chaos", "400", "--no-reuse"]).unwrap().unwrap();
        assert_eq!(options.chaos_frames, Some(400));
        assert!(!options.reuse_previous);
    }
}
