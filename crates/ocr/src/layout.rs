//! Lines grouped into classified blocks with sampled colours.
//!
//! [`analyze`] takes the lines of one screen in any order and returns the blocks an overlay is
//! composed from: consecutive lines that share a plate and a language group together, every block
//! is classified from its text size and aspect ratio, and its background and text colours are
//! sampled from the frame. Reading order is preserved throughout.

use std::collections::HashMap;

use lumen_core::{Frame, Rect};

use crate::language::LanguageId;
use crate::source::TextSpan;

/// A straight (non-premultiplied) BGRA colour, the byte order of [`Frame::pixel`].
pub type Color = [u8; 4];

/// The plate a region is painted on when the frame holds no pixels for it.
pub const TRANSPARENT: Color = [0, 0, 0, 0];

/// The typeface a line is set in. Recognised lines only know an estimated size; UI Automation
/// lines will carry the real family and weight once that source lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    /// Size of the glyphs in pixels, roughly the em height.
    pub size: u32,
    pub bold: bool,
    pub family: Option<String>,
}

impl Font {
    /// The size for a line whose glyphs cover `line_height` pixels — the recognition case, where
    /// no font metrics exist and the line bounds are all there is.
    pub fn estimated(line_height: u32) -> Self {
        Self {
            size: line_height * 3 / 4,
            bold: false,
            family: None,
        }
    }
}

/// One line of on-screen text: where it sits, what it sits on, and how it is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub bounds: Rect,
    pub background: Color,
    pub text: String,
    pub font: Font,
    pub language: LanguageId,
}

impl Line {
    /// Builds a line for `span`, taking its plate colour from `frame`. Regions without a single
    /// non-transparent pixel report a transparent plate.
    pub fn from_span(frame: &Frame, span: &TextSpan, font: Font) -> Self {
        let background = sample_colors(frame, span.bounds)
            .map(|colors| colors.background)
            .unwrap_or(TRANSPARENT);
        Self {
            bounds: span.bounds,
            background,
            text: span.text.clone(),
            font,
            language: span.language,
        }
    }
}

/// The two dominant colours of a region: the plate it is painted on and the ink on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub background: Color,
    pub text: Color,
}

/// Samples the colours of `bounds` in `frame`: the most frequent non-transparent pixel is the
/// background, the second most frequent is the text colour. A flat region reports its only colour
/// twice; a region without a single non-transparent pixel reports [`None`].
pub fn sample_colors(frame: &Frame, bounds: Rect) -> Option<Colors> {
    let region = bounds.clamp_to(&frame.bounds())?;
    let mut counts: HashMap<Color, u64> = HashMap::new();
    for y in region.y as u32..region.bottom() as u32 {
        for x in region.x as u32..region.right() as u32 {
            let pixel = frame.pixel(x, y);
            if pixel[3] != 0 {
                *counts.entry(pixel).or_insert(0) += 1;
            }
        }
    }
    let mut ranked: Vec<(Color, u64)> = counts.into_iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    let background = ranked.first()?.0;
    Some(Colors {
        background,
        text: ranked.get(1).map(|entry| entry.0).unwrap_or(background),
    })
}

/// What a block of text is doing on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Button,
    Tooltip,
    Dialogue,
    Menu,
    Subtitle,
    Body,
}

/// A group of lines on one plate: what it is, where it sits, its colours, its language, and the
/// lines that make it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub bounds: Rect,
    pub background: Color,
    pub text_color: Color,
    pub language: LanguageId,
    pub lines: Vec<Line>,
}

impl Block {
    /// The block's text, one entry per line.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Groups `lines` into blocks and classifies them, in reading order.
///
/// Consecutive lines join the same block when they share its language and plate colour, overlap
/// horizontally and sit within half a line of leading of each other; anything else starts a new
/// block. The block's colours are sampled from `frame` over its bounds — the most frequent
/// non-transparent pixel is the background, the second most frequent the text colour — and its
/// kind is classified from the largest font size among its lines and the aspect ratio of its
/// bounds. Blocks come back top to bottom, left to right, and so do the lines inside them.
pub fn analyze(frame: &Frame, lines: Vec<Line>) -> Vec<Block> {
    let mut lines = lines;
    lines.sort_by(reading_order);
    let mut groups: Vec<Vec<Line>> = Vec::new();
    for line in lines {
        match groups.last_mut() {
            Some(group) if joins_previous(group, &line) => group.push(line),
            _ => groups.push(vec![line]),
        }
    }
    groups.into_iter().map(|lines| block_of(frame, lines)).collect()
}

fn block_of(frame: &Frame, lines: Vec<Line>) -> Block {
    let mut bounds = lines[0].bounds;
    for line in &lines[1..] {
        bounds = bounds.union(&line.bounds);
    }
    let size = lines.iter().map(|line| line.font.size).max().unwrap_or(0);
    let aspect = bounds.width as f32 / bounds.height as f32;
    let colors = sample_colors(frame, bounds).unwrap_or(Colors {
        background: TRANSPARENT,
        text: TRANSPARENT,
    });
    Block {
        kind: classify(size, aspect),
        bounds,
        background: colors.background,
        text_color: colors.text,
        language: lines[0].language,
        lines,
    }
}

/// Classifies a block from the size its text is set at and the aspect ratio of its bounds: large
/// wide text is a subtitle, other large text a dialogue, medium boxy text body copy, other medium
/// text a button, small boxy text a menu, and the rest a tooltip. The bands — 18 and 24 pixels of
/// text, aspect ratios of 1.5 and 3 — are tuned for desktop UI around 1080p and get revisited
/// against real screens with the golden images in M4.
fn classify(size: u32, aspect: f32) -> BlockKind {
    if size >= 24 {
        return if aspect >= 3.0 {
            BlockKind::Subtitle
        } else {
            BlockKind::Dialogue
        };
    }
    if size >= 18 {
        return if aspect >= 1.5 {
            BlockKind::Button
        } else {
            BlockKind::Body
        };
    }
    if aspect >= 1.5 {
        BlockKind::Tooltip
    } else {
        BlockKind::Menu
    }
}

fn joins_previous(group: &[Line], line: &Line) -> bool {
    let previous = &group[group.len() - 1];
    let gap = i64::from(line.bounds.y) - i64::from(previous.bounds.bottom());
    line.language == previous.language
        && line.background == previous.background
        && overlap_horizontally(&line.bounds, &previous.bounds)
        && gap <= i64::from(previous.font.size) / 2
}

fn overlap_horizontally(left: &Rect, right: &Rect) -> bool {
    left.x < right.right() && right.x < left.right()
}

fn reading_order(left: &Line, right: &Line) -> std::cmp::Ordering {
    left.bounds
        .y
        .cmp(&right.bounds.y)
        .then(left.bounds.x.cmp(&right.bounds.x))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::TextOrigin;

    const PLATE: Color = [40, 30, 20, 255];
    const INK: Color = [230, 220, 210, 255];

    fn line(text: &str, bounds: Rect, size: u32) -> Line {
        Line {
            bounds,
            background: PLATE,
            text: text.to_owned(),
            font: Font {
                size,
                bold: false,
                family: None,
            },
            language: LanguageId::En,
        }
    }

    fn plated_frame(width: u32, height: u32) -> Frame {
        Frame::filled(width, height, PLATE).unwrap()
    }

    #[test]
    fn sample_colors_ranks_the_plate_first_and_the_ink_second() {
        let mut frame = plated_frame(10, 10);
        frame.fill_rect(Rect::new(2, 2, 2, 2), INK);
        frame.fill_rect(Rect::new(8, 8, 1, 1), [0, 0, 0, 0]);
        let colors = sample_colors(&frame, frame.bounds()).unwrap();
        assert_eq!(colors.background, PLATE);
        assert_eq!(colors.text, INK);
    }

    #[test]
    fn sample_colors_reports_a_flat_region_as_one_colour() {
        let frame = plated_frame(6, 6);
        let colors = sample_colors(&frame, Rect::new(1, 1, 4, 4)).unwrap();
        assert_eq!(colors.background, PLATE);
        assert_eq!(colors.text, PLATE);
    }

    #[test]
    fn sample_colors_finds_nothing_outside_the_frame() {
        let frame = plated_frame(6, 6);
        assert_eq!(sample_colors(&frame, Rect::new(20, 20, 4, 4)), None);
    }

    #[test]
    fn a_line_carries_the_plate_of_its_region() {
        let frame = plated_frame(10, 10);
        let span = TextSpan {
            text: "Menu".to_owned(),
            language: LanguageId::En,
            origin: TextOrigin::Ocr,
            bounds: Rect::new(1, 1, 6, 4),
            confidence: 0.9,
        };
        let line = Line::from_span(&frame, &span, Font::estimated(4));
        assert_eq!(line.background, PLATE);
        assert_eq!(line.bounds, Rect::new(1, 1, 6, 4));
        assert_eq!(line.text, "Menu");
        assert_eq!(line.font.size, 3);
    }

    #[test]
    fn each_size_and_aspect_band_names_its_kind() {
        let frame = plated_frame(800, 600);
        let cases = [
            (line("subtitle", Rect::new(0, 0, 600, 20), 28), BlockKind::Subtitle),
            (line("dialogue", Rect::new(0, 0, 200, 120), 28), BlockKind::Dialogue),
            (line("body", Rect::new(0, 0, 200, 150), 20), BlockKind::Body),
            (line("button", Rect::new(0, 0, 120, 24), 20), BlockKind::Button),
            (line("menu", Rect::new(0, 0, 100, 80), 14), BlockKind::Menu),
            (line("tooltip", Rect::new(0, 0, 240, 40), 14), BlockKind::Tooltip),
        ];
        for (line, expected) in cases {
            let blocks = analyze(&frame, vec![line]);
            assert_eq!(blocks[0].kind, expected);
        }
    }

    #[test]
    fn stacked_lines_on_one_plate_form_one_block() {
        let frame = plated_frame(800, 600);
        let blocks = analyze(
            &frame,
            vec![
                line("second", Rect::new(10, 30, 100, 20), 18),
                line("first", Rect::new(10, 4, 100, 20), 18),
            ],
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].bounds, Rect::new(10, 4, 100, 46));
        assert_eq!(blocks[0].text(), "first\nsecond");
        assert_eq!(blocks[0].kind, BlockKind::Button);
    }

    #[test]
    fn lines_split_on_a_plate_a_language_or_a_gap() {
        let frame = plated_frame(800, 600);
        let top = line("top", Rect::new(10, 0, 100, 20), 18);
        let mut other_plate = line("cold", Rect::new(10, 26, 100, 20), 18);
        other_plate.background = INK;
        let mut other_language = line("warm", Rect::new(10, 26, 100, 20), 18);
        other_language.language = LanguageId::Ru;
        let far = line("far", Rect::new(10, 60, 100, 20), 18);
        assert_eq!(analyze(&frame, vec![top.clone(), other_plate]).len(), 2);
        assert_eq!(analyze(&frame, vec![top.clone(), other_language]).len(), 2);
        assert_eq!(analyze(&frame, vec![top, far]).len(), 2);
    }

    #[test]
    fn a_block_samples_its_colours_and_keeps_its_language() {
        let mut frame = plated_frame(800, 600);
        frame.fill_rect(Rect::new(20, 10, 8, 8), INK);
        let mut text = line("Привет", Rect::new(10, 4, 100, 20), 18);
        text.language = LanguageId::Ru;
        let blocks = analyze(&frame, vec![text]);
        assert_eq!(blocks[0].background, PLATE);
        assert_eq!(blocks[0].text_color, INK);
        assert_eq!(blocks[0].language, LanguageId::Ru);
    }

    #[test]
    fn recognised_text_flows_from_spans_to_one_block() {
        let frame = plated_frame(800, 600);
        let spans = [
            TextSpan {
                text: "New game".to_owned(),
                language: LanguageId::En,
                origin: TextOrigin::Ocr,
                bounds: Rect::new(10, 4, 120, 24),
                confidence: 0.9,
            },
            TextSpan {
                text: "Continue".to_owned(),
                language: LanguageId::En,
                origin: TextOrigin::Ocr,
                bounds: Rect::new(10, 32, 120, 24),
                confidence: 0.9,
            },
        ];
        let lines: Vec<Line> = spans
            .iter()
            .map(|span| Line::from_span(&frame, span, Font::estimated(span.bounds.height)))
            .collect();
        let blocks = analyze(&frame, lines);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Button);
        assert_eq!(blocks[0].text(), "New game\nContinue");
    }
}
