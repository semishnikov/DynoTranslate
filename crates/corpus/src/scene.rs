//! One corpus scene: a frame of text and the truth about it.
//!
//! A scene is composed from its spec and nothing else — seed, language, style, background kind and
//! size decide every pixel and every ground-truth box. The box of a line is measured from the ink
//! the renderer actually laid down, not promised from the metrics, so the transcript a recognition
//! engine is scored against describes the picture that was drawn.

use lumen_core::{Frame, Rect};
use lumen_language::Language;
use serde::{Deserialize, Serialize};

use crate::background::{Background, BackgroundKind};
use crate::font::FontStyle;
use crate::phrases::phrases_for;
use crate::render::{self, TextStyle};
use crate::rng::Rng;
use crate::CorpusError;

/// Ink and panel colours the scenes alternate between, as BGRA like every frame byte order here.
/// Every pair is a legibility contrast in one direction or the other: light ink on a dark panel or
/// dark ink on a light one, which is the split real interfaces have.
const PALETTES: &[([u8; 4], [u8; 4])] = &[
    ([232, 230, 226, 255], [30, 26, 24, 255]),
    ([214, 228, 236, 255], [22, 30, 40, 255]),
    ([26, 24, 22, 255], [232, 228, 220, 255]),
    ([240, 236, 228, 255], [54, 42, 30, 255]),
    ([20, 26, 20, 255], [210, 224, 206, 255]),
    ([236, 224, 240, 255], [36, 26, 44, 255]),
];

/// How far a gradient moves away from the panel colour at each end.
const GRADIENT_SPREAD: i16 = 18;
/// Extra air between the descent of one line and the ascent of the next, in units of the size.
const LEADING: f32 = 0.55;
/// The size the shrink-to-fit loop stops at, however much text is still too wide.
const MINIMUM_SIZE: f32 = 5.0;

/// Everything that decides one scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneSpec {
    pub name: String,
    pub seed: u64,
    pub language: Language,
    pub style: FontStyle,
    pub background: BackgroundKind,
    pub width: u32,
    pub height: u32,
    /// How many lines the scene wants; it takes fewer only when they cannot fit.
    pub lines: usize,
}

/// One line of ground truth: the text, and the box its ink actually occupies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundLine {
    pub text: String,
    pub bounds: Rect,
    pub language: Language,
}

/// A composed scene: the frame, and the truth a recognition engine is measured against.
#[derive(Debug, Clone)]
pub struct CorpusScene {
    pub name: String,
    pub language: Language,
    pub style: FontStyle,
    pub background: Background,
    pub frame: Frame,
    pub lines: Vec<GroundLine>,
}

impl CorpusScene {
    /// The whole scene as one passage, the form language identification sees.
    pub fn transcript(&self) -> String {
        let mut texts = Vec::with_capacity(self.lines.len());
        for line in &self.lines {
            texts.push(line.text.as_str());
        }
        texts.join(" ")
    }
}

/// Composes the scene a spec describes. The same spec always yields the same pixels.
pub fn compose(spec: &SceneSpec) -> Result<CorpusScene, CorpusError> {
    if spec.lines == 0 {
        return Err(CorpusError::BadArgument("a scene needs at least one line".to_owned()));
    }
    let phrases = match phrases_for(spec.language) {
        Some(list) => list,
        None => {
            let message = format!("the corpus has no phrase pack for {:?}", spec.language);
            return Err(CorpusError::BadArgument(message));
        }
    };

    let mut rng = Rng::new(spec.seed);
    let (foreground, panel) = PALETTES[rng.pick(PALETTES.len() as u32) as usize];
    let background = background_of(spec, panel, &mut rng);

    let mut order: Vec<usize> = (0..phrases.len()).collect();
    rng.shuffle(&mut order);
    let wanted = spec.lines.min(phrases.len());
    let mut chosen = Vec::with_capacity(wanted);
    for index in order.into_iter().take(wanted) {
        chosen.push(phrases[index]);
    }

    let margin_x = (spec.width / 16).max(16);
    let margin_y = (spec.height / 16).max(16);
    let usable_width = spec.width.saturating_sub(2 * margin_x).max(1) as f32;
    let text_style = TextStyle {
        size: 20.0,
        style: spec.style,
        color: foreground,
    };

    let mut size = spec.height as f32 * (0.05 + 0.02 * rng.unit());
    loop {
        let probe = TextStyle { size, ..text_style };
        let widest = widest_line(&chosen, &probe)?;
        let ascent = render::measure("Hh", &probe)?.ascent;
        let descent = render::measure("Hh", &probe)?.descent;
        let needed = ascent + descent + (wanted - 1) as f32 * (ascent + descent + LEADING * size)
            + 2.0 * margin_y as f32;
        if (widest <= usable_width && needed <= spec.height as f32) || size <= MINIMUM_SIZE {
            break;
        }
        size *= 0.85;
    }

    let probe = TextStyle { size, ..text_style };
    let ascent = render::measure("Hh", &probe)?.ascent;
    let descent = render::measure("Hh", &probe)?.descent;
    let pitch = ascent + descent + LEADING * size;
    let room = spec.height as f32 - 2.0 * margin_y as f32 - ascent - descent;
    // No room at all still keeps one line: the clamp below the max is the plan's floor of one.
    let capacity = 1 + (room / pitch).max(0.0).floor() as usize;
    let line_count = chosen.len().min(capacity.max(1));

    let mut frame = Frame::filled(spec.width, spec.height, panel)?;
    background.paint(&mut frame);

    let mut lines = Vec::with_capacity(line_count);
    let mut baseline = margin_y as f32 + ascent;
    for phrase in chosen.iter().take(line_count) {
        let bounds = render::draw_line(&mut frame, margin_x as f32, baseline, phrase, &probe)?;
        lines.push(GroundLine {
            text: (*phrase).to_owned(),
            bounds,
            language: spec.language,
        });
        baseline += pitch;
    }

    Ok(CorpusScene {
        name: spec.name.clone(),
        language: spec.language,
        style: spec.style,
        background,
        frame,
        lines,
    })
}

fn widest_line(phrases: &[&str], text_style: &TextStyle) -> Result<f32, CorpusError> {
    let mut widest: f32 = 0.0;
    for phrase in phrases {
        widest = widest.max(render::measure(phrase, text_style)?.width);
    }
    Ok(widest)
}

fn background_of(spec: &SceneSpec, panel: [u8; 4], rng: &mut Rng) -> Background {
    match spec.background {
        BackgroundKind::Solid => Background::solid(panel),
        BackgroundKind::Gradient => Background::Gradient {
            top: shade(panel, GRADIENT_SPREAD),
            bottom: shade(panel, -GRADIENT_SPREAD),
        },
        BackgroundKind::Noise => Background::Noise {
            base: panel,
            amplitude: 8 + rng.pick(9) as u8,
            seed: spec.seed ^ 0x5DEE_CE66_D309_FE2D,
        },
    }
}

/// Moves the colour channels of `color` by `delta`, leaving alpha alone.
fn shade(color: [u8; 4], delta: i16) -> [u8; 4] {
    let mut shaded = color;
    for channel in shaded.iter_mut().take(3) {
        *channel = (i16::from(*channel) + delta).clamp(0, 255) as u8;
    }
    shaded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::CorpusPlan;

    fn spec(name: &str, seed: u64, language: Language, style: FontStyle, background: BackgroundKind) -> SceneSpec {
        SceneSpec {
            name: name.to_owned(),
            seed,
            language,
            style,
            background,
            width: 400,
            height: 260,
            lines: 4,
        }
    }

    #[test]
    fn composing_is_deterministic() {
        let base = spec("a", 11, Language::Russian, FontStyle::Bold, BackgroundKind::Noise);
        let left = compose(&base).unwrap();
        let right = compose(&base).unwrap();
        assert_eq!(left.frame, right.frame);
        assert_eq!(left.lines, right.lines);
        assert_eq!(left.transcript(), right.transcript());
    }

    #[test]
    fn every_line_stays_inside_the_frame_and_apart_from_its_neighbours() {
        for scene_spec in CorpusPlan::standard(400, 260).scenes().iter().take(27) {
            let scene = compose(scene_spec).unwrap();
            let bounds = scene.frame.bounds();
            assert!(!scene.lines.is_empty(), "{scene_spec:?} produced no lines");
            for (index, line) in scene.lines.iter().enumerate() {
                assert!(!line.text.is_empty());
                let sized = line.bounds.width > 0 && line.bounds.height > 0;
                assert!(sized, "{:?}: {:?}", scene_spec.name, line);
                assert!(bounds.contains(line.bounds.x, line.bounds.y), "{:?}", line.bounds);
                assert!(
                    bounds.contains(line.bounds.right() - 1, line.bounds.bottom() - 1),
                    "{:?}",
                    line.bounds
                );
                for other in scene.lines.iter().skip(index + 1) {
                    assert!(
                        !line.bounds.intersects(&other.bounds),
                        "{:?} overlaps {:?}",
                        line.text,
                        other.text
                    );
                }
            }
        }
    }

    #[test]
    fn the_scene_keeps_what_the_spec_asked_for() {
        let kept = spec("kep", 5, Language::English, FontStyle::Italic, BackgroundKind::Gradient);
        let scene = compose(&kept).unwrap();
        assert_eq!(scene.name, "kep");
        assert_eq!(scene.language, Language::English);
        assert_eq!(scene.style, FontStyle::Italic);
        assert_eq!(scene.background.kind(), BackgroundKind::Gradient);
        assert_eq!(scene.lines.len(), 4);
        assert!(scene.lines.iter().all(|line| line.language == Language::English));
    }

    #[test]
    fn the_transcript_carries_every_line() {
        let base = spec("t", 3, Language::German, FontStyle::Regular, BackgroundKind::Solid);
        let scene = compose(&base).unwrap();
        let transcript = scene.transcript();
        for line in &scene.lines {
            assert!(transcript.contains(&line.text));
        }
    }

    #[test]
    fn a_scene_with_no_room_still_keeps_one_line_inside_the_frame() {
        let mut tiny = spec("tiny", 9, Language::French, FontStyle::Regular, BackgroundKind::Solid);
        tiny.width = 120;
        tiny.height = 40;
        let scene = compose(&tiny).unwrap();
        assert!(!scene.lines.is_empty());
        let bounds = scene.frame.bounds();
        for line in &scene.lines {
            assert!(bounds.contains(line.bounds.x, line.bounds.y));
            let inside = line.bounds.right() <= bounds.right() && line.bounds.bottom() <= bounds.bottom();
            assert!(inside);
        }
    }

    #[test]
    fn specs_the_corpus_cannot_honour_are_errors() {
        let mut empty = spec("empty", 1, Language::English, FontStyle::Regular, BackgroundKind::Solid);
        empty.lines = 0;
        assert!(matches!(compose(&empty), Err(CorpusError::BadArgument(_))));

        let unsupported = spec("knj", 1, Language::Japanese, FontStyle::Regular, BackgroundKind::Solid);
        assert!(matches!(compose(&unsupported), Err(CorpusError::BadArgument(_))));
    }

    #[test]
    fn shading_leaves_alpha_alone() {
        let shaded = shade([10, 250, 128, 77], 20);
        assert_eq!(shaded, [30, 255, 148, 77]);
        let darkened = shade([10, 250, 128, 77], -20);
        assert_eq!(darkened, [0, 230, 108, 77]);
    }
}
