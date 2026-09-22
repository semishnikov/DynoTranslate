//! Collecting lines into blocks and saying what kind of interface element each one is.
//!
//! A block is the unit translation works on: a dialogue box is translated as one piece of prose, a
//! menu entry as one short label, a subtitle under its own length rules. Only geometry and colour
//! are used to decide, because those are the signals available without a display, and because a
//! classification that rested on anything else could not be pinned by a test.

use crate::colour::{modal_colour, near, sample_pair};
use crate::lines::{group_lines, Line};
use crate::LayoutConfig;
use lumen_core::{Frame, Rect};
use lumen_source::TextRun;
use serde::{Deserialize, Serialize};
use std::mem;

/// What a block is in the interface, which decides how its translation is worded and fitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockKind {
    /// A control the user activates: short text on a fill of its own.
    Button,
    /// One entry of a vertical list of choices.
    MenuItem,
    /// A few lines floating over the content, usually explaining whatever is under the cursor.
    Tooltip,
    /// A passage of prose the characters are speaking.
    Dialogue,
    /// Narration or speech pinned to the bottom of the frame.
    Subtitle,
    /// Anything else: a caption, a label, a heading.
    Label,
}

/// How a block sits within the window. The compositor matches it when drawing the translation, so a
/// centred subtitle stays centred instead of jumping to the left edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// One translatable unit: the lines it holds, where they are, and how to draw over them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    /// The smallest box holding every line, in frame pixels.
    pub bounds: Rect,
    pub lines: Vec<Line>,
    pub alignment: Alignment,
    /// The surface the text sits on, measured from a ring just outside it.
    pub background: [u8; 4],
    /// The colour the text is drawn in.
    pub foreground: [u8; 4],
    /// Estimated glyph height in pixels: the median of the block's line heights.
    pub font_size: u32,
    /// Measured stroke weight, so the renderer can match bold interface text with a bold face.
    pub weight: StrokeWeight,
    /// Lowest confidence among the block's runs.
    pub confidence: f32,
}

/// Stroke weight the renderer should draw with. Measured from how much of the block the ink
/// covers rather than guessed from the font name, which recognition never reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StrokeWeight {
    #[default]
    Regular,
    Bold,
}

impl Block {
    /// Every line separated the way prose is, so the translator sees one passage rather than a list
    /// of fragments.
    pub fn text(&self) -> String {
        let parts: Vec<String> = self.lines.iter().map(|line| line.text()).collect();
        parts.join("\n")
    }

    /// Characters across all lines, which is what tells a control apart from a paragraph.
    pub fn char_count(&self) -> usize {
        self.lines
            .iter()
            .flat_map(|line| &line.runs)
            .map(|run| run.text.chars().count())
            .sum()
    }

    /// Which kind of element this block is. Position in the window decides first, because a subtitle
    /// at the bottom is a subtitle whatever it looks like; then membership of a menu; then the fill a
    /// control sits on.
    fn classify(&self, context: Context, menu: bool) -> BlockKind {
        let config = context.config;
        let single = self.lines.len() == 1;
        let width = self.bounds.width as f32 / context.page.width.max(1) as f32;
        let bottom = context.page.bottom();
        let above_bottom = (bottom - self.bounds.bottom()) as f32 / context.page.height.max(1) as f32;
        let centred_wide = self.alignment == Alignment::Center && width >= config.subtitle_min_width;
        let dialogue_shape = above_bottom <= config.dialogue_band && width >= config.dialogue_min_width;
        let on_own_fill = self.char_count() <= config.button_text_limit && !near(self.background, context.dominant);

        if !single && centred_wide && above_bottom <= config.subtitle_band {
            return BlockKind::Subtitle;
        }
        if !single && dialogue_shape {
            return BlockKind::Dialogue;
        }
        if menu {
            return BlockKind::MenuItem;
        }
        if single && on_own_fill {
            return BlockKind::Button;
        }
        if !single && width < config.tooltip_max_width {
            return BlockKind::Tooltip;
        }
        BlockKind::Label
    }
}

/// The window a block is being classified against: its extent, the colour most of it is painted in,
/// and the thresholds in force.
#[derive(Clone, Copy)]
struct Context<'a> {
    page: Rect,
    dominant: [u8; 4],
    config: &'a LayoutConfig,
}

impl<'a> Context<'a> {
    fn new(frame: &Frame, config: &'a LayoutConfig) -> Self {
        let page = frame.bounds();
        let dominant = modal_colour(frame, page);
        Self { page, dominant, config }
    }
}

/// Groups recognised runs into classified blocks, in reading order.
///
/// `frame` is only read, to measure the colours each block sits on and is drawn in.
pub fn analyse(frame: &Frame, runs: Vec<TextRun>, config: &LayoutConfig) -> Vec<Block> {
    let context = Context::new(frame, config);
    let groups = group_blocks(group_lines(runs, config), config);

    let mut blocks: Vec<Block> = Vec::with_capacity(groups.len());
    for group in groups {
        blocks.push(build(frame, group, context));
    }
    classify_blocks(&mut blocks, context);
    blocks
}

/// Collects lines into blocks: consecutive lines that sit close vertically and share a column belong
/// to one paragraph, list or dialogue box.
pub fn group_blocks(lines: Vec<Line>, config: &LayoutConfig) -> Vec<Vec<Line>> {
    let mut blocks: Vec<Vec<Line>> = Vec::new();
    let mut current: Vec<Line> = Vec::new();
    for line in lines {
        if !current.is_empty() && !continues_block(&current, &line, config) {
            blocks.push(mem::take(&mut current));
        }
        current.push(line);
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

/// Whether `line` continues the block `block` already holds: it sits no further below the previous
/// line than [`LayoutConfig::max_line_spacing`] line heights, and it shares enough of its width to
/// be part of the same column of text.
fn continues_block(block: &[Line], line: &Line, config: &LayoutConfig) -> bool {
    let Some(previous) = block.last() else {
        return false;
    };

    let gap = line.bounds.y - previous.bounds.bottom();
    if (gap as f32) > (previous.bounds.height.max(1) as f32) * config.max_line_spacing {
        return false;
    }

    let left = previous.bounds.x.max(line.bounds.x);
    let right = previous.bounds.right().min(line.bounds.right());
    let shared = (right - left).max(0);
    let narrower = previous.bounds.width.min(line.bounds.width).max(1);
    (shared as f32) >= (narrower as f32) * config.min_horizontal_overlap
}

fn build(frame: &Frame, lines: Vec<Line>, context: Context) -> Block {
    let bounds = union_of(&lines);
    let (background, foreground) = sample_pair(frame, bounds);
    let alignment = alignment_of(bounds, context);
    let font_size = median_height(&lines);
    let weight = estimate_weight(frame, bounds, background, foreground);
    let confidence = lines.iter().map(|line| line.confidence()).fold(1.0, f32::min);
    Block {
        kind: BlockKind::Label,
        bounds,
        lines,
        alignment,
        background,
        foreground,
        font_size,
        weight,
        confidence,
    }
}

/// How much of the box the glyphs cover, which is what tells a bold face from a regular one
/// without a font name. Below [`BOLD_COVERAGE`] the strokes are thin; above it they are not.
const BOLD_COVERAGE: f32 = 0.22;

fn estimate_weight(
    frame: &Frame,
    bounds: Rect,
    background: [u8; 4],
    foreground: [u8; 4],
) -> StrokeWeight {
    let Some(region) = bounds.clamp_to(&frame.bounds()) else {
        return StrokeWeight::Regular;
    };
    if region.is_empty() {
        return StrokeWeight::Regular;
    }
    let mut ink = 0_u32;
    let mut total = 0_u32;
    // Sample on a stride that keeps a full-screen block at a few thousand probes.
    let step = ((region.width.max(region.height) / 64) as usize).max(1);
    for y in (region.y..region.bottom()).step_by(step) {
        for x in (region.x..region.right()).step_by(step) {
            total += 1;
            let pixel = frame.pixel(x as u32, y as u32);
            if !crate::colour::near(pixel, background) && crate::colour::near(pixel, foreground) {
                ink += 1;
            }
        }
    }
    if total == 0 {
        return StrokeWeight::Regular;
    }
    let coverage = ink as f32 / total as f32;
    if coverage >= BOLD_COVERAGE {
        StrokeWeight::Bold
    } else {
        StrokeWeight::Regular
    }
}

/// Applies the classification rules to every block once they all exist, because whether a block is a
/// menu entry depends on the blocks around it.
fn classify_blocks(blocks: &mut [Block], context: Context) {
    let stacked = menu_stack_indices(blocks, context);
    for (index, block) in blocks.iter_mut().enumerate() {
        let kind = block.classify(context, stacked.contains(&index));
        block.kind = kind;
    }
}

/// Indices of the blocks that form a menu: single lines stacked in one column, at least
/// [`LayoutConfig::menu_stack_minimum`] of them.
fn menu_stack_indices(blocks: &[Block], context: Context) -> Vec<usize> {
    let singles: Vec<usize> = blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| block.lines.len() == 1)
        .map(|(index, _)| index)
        .collect();

    let mut chains: Vec<Vec<usize>> = Vec::new();
    let mut chain: Vec<usize> = Vec::new();
    for index in singles {
        let joins = chain
            .last()
            .is_some_and(|previous| stacks(&blocks[*previous], &blocks[index], context));
        if !joins && !chain.is_empty() {
            chains.push(mem::take(&mut chain));
        }
        chain.push(index);
    }
    if !chain.is_empty() {
        chains.push(chain);
    }

    let mut stacked: Vec<usize> = Vec::new();
    for chain in chains {
        if chain.len() >= context.config.menu_stack_minimum {
            stacked.extend(chain);
        }
    }
    stacked
}

/// Whether `below` is the next entry of the menu `above` belongs to: same left edge, and close
/// enough below it to read as one list rather than two unrelated labels.
fn stacks(above: &Block, below: &Block, context: Context) -> bool {
    let height = above.bounds.height.max(1) as f32;
    let drift = (above.bounds.x - below.bounds.x).abs();
    if (drift as f32) > height * context.config.menu_edge_tolerance {
        return false;
    }
    ((below.bounds.y - above.bounds.bottom()) as f32) <= height * context.config.menu_gap
}

fn union_of(lines: &[Line]) -> Rect {
    let mut bounds = lines.first().map_or(Rect::new(0, 0, 0, 0), |line| line.bounds);
    for line in lines.iter().skip(1) {
        bounds = bounds.union(&line.bounds);
    }
    bounds
}

fn median_height(lines: &[Line]) -> u32 {
    let mut heights: Vec<u32> = lines.iter().map(|line| line.bounds.height).collect();
    heights.sort_unstable();
    heights.get(heights.len() / 2).copied().unwrap_or(0)
}

/// Where a block sits relative to the window rather than to its neighbours, so the answer does not
/// change when an unrelated label appears somewhere else on screen.
fn alignment_of(bounds: Rect, context: Context) -> Alignment {
    let page = context.page;
    let tolerance = page.width as f32 * context.config.alignment_tolerance;
    let centre = (bounds.x + bounds.right()) as f32 / 2.0;
    let page_centre = (page.x + page.right()) as f32 / 2.0;
    if (centre - page_centre).abs() <= tolerance {
        return Alignment::Center;
    }
    if ((page.right() - bounds.right()).abs() as f32) <= tolerance {
        return Alignment::Right;
    }
    Alignment::Left
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_source::SourceKind;

    const SURFACE: [u8; 4] = [30, 30, 30, 255];
    const INK: [u8; 4] = [235, 235, 235, 255];
    const FILL: [u8; 4] = [80, 40, 200, 255];

    fn scene(width: u32, height: u32) -> Frame {
        Frame::filled(width, height, SURFACE).expect("valid dimensions")
    }

    fn run(text: &str, x: i32, y: i32, width: u32) -> TextRun {
        TextRun::new(text, Rect::new(x, y, width, 16), SourceKind::Ocr, 0.9)
    }

    fn line(text: &str, x: i32, y: i32, width: u32, height: u32) -> Line {
        Line::single(TextRun::new(text, Rect::new(x, y, width, height), SourceKind::Ocr, 0.9))
    }

    fn config() -> LayoutConfig {
        LayoutConfig::default()
    }

    #[test]
    fn close_lines_sharing_a_column_form_one_block() {
        let lines = vec![line("one", 20, 20, 100, 16), line("two", 20, 38, 100, 16)];

        assert_eq!(group_blocks(lines, &config()).len(), 1);
    }

    #[test]
    fn a_gap_of_more_than_one_line_starts_a_new_block() {
        let lines = vec![line("one", 20, 20, 100, 16), line("two", 20, 90, 100, 16)];

        assert_eq!(group_blocks(lines, &config()).len(), 2);
    }

    #[test]
    fn lines_in_different_columns_do_not_share_a_block() {
        let lines = vec![line("one", 20, 20, 60, 16), line("two", 300, 38, 60, 16)];

        assert_eq!(group_blocks(lines, &config()).len(), 2);
    }

    #[test]
    fn blocks_come_back_top_to_bottom() {
        let frame = scene(640, 360);
        let runs = vec![run("низ", 80, 300, 60), run("верх", 80, 40, 60)];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text(), "верх");
        assert_eq!(blocks[1].text(), "низ");
    }

    #[test]
    fn a_wide_centred_block_at_the_bottom_is_a_subtitle() {
        let frame = scene(640, 360);
        let runs = vec![run("первая", 180, 300, 280), run("вторая", 190, 322, 260)];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Subtitle);
        assert_eq!(blocks[0].alignment, Alignment::Center);
    }

    #[test]
    fn a_wide_multi_line_block_low_in_the_frame_is_dialogue() {
        let frame = scene(640, 360);
        let runs = vec![
            run("первая", 60, 220, 400),
            run("вторая", 60, 242, 380),
            run("третья", 60, 264, 360),
        ];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Dialogue);
        assert_eq!(blocks[0].alignment, Alignment::Left);
    }

    #[test]
    fn a_narrow_multi_line_block_is_a_tooltip() {
        let frame = scene(640, 360);
        let runs = vec![run("Удерживайте", 400, 60, 120), run("подробности", 400, 82, 120)];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks[0].kind, BlockKind::Tooltip);
    }

    #[test]
    fn a_stack_of_short_lines_sharing_an_edge_is_a_menu() {
        let frame = scene(640, 360);
        let runs = vec![
            run("Новая игра", 80, 100, 120),
            run("Загрузить", 80, 140, 120),
            run("Настройки", 80, 180, 120),
            run("Выход", 80, 220, 120),
        ];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks.len(), 4);
        assert!(blocks.iter().all(|block| block.kind == BlockKind::MenuItem));
    }

    #[test]
    fn a_short_line_on_a_fill_of_its_own_is_a_button() {
        let mut frame = scene(640, 360);
        frame.fill_rect(Rect::new(240, 150, 160, 40), FILL);
        let runs = vec![run("Продолжить", 270, 162, 100)];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Button);
        assert_eq!(blocks[0].background, FILL);
    }

    #[test]
    fn a_single_line_on_the_page_background_is_a_label() {
        let frame = scene(640, 360);
        let runs = vec![run("Версия 1.4.2", 24, 320, 110)];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks[0].kind, BlockKind::Label);
    }

    #[test]
    fn a_block_is_flush_with_the_edge_it_is_nearest_to() {
        let frame = scene(640, 360);
        let runs = vec![run("справа", 500, 40, 120)];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks[0].alignment, Alignment::Right);
    }

    #[test]
    fn a_block_reports_the_median_line_height_and_the_weakest_confidence() {
        let frame = scene(640, 360);
        let weak = TextRun::new("слабое", Rect::new(60, 220, 200, 14), SourceKind::Ocr, 0.3);
        let strong = TextRun::new("сильное", Rect::new(60, 244, 200, 20), SourceKind::Ocr, 0.95);

        let blocks = analyse(&frame, vec![strong, weak], &config());

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].font_size, 20);
        assert!((blocks[0].confidence - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn a_block_carries_the_measured_surface_and_ink() {
        let mut frame = scene(640, 360);
        frame.fill_rect(Rect::new(60, 220, 200, 16), INK);
        let runs = vec![run("текст", 60, 220, 200)];

        let blocks = analyse(&frame, runs, &config());

        assert_eq!(blocks[0].background, SURFACE);
        assert_eq!(blocks[0].foreground, INK);
    }

    #[test]
    fn an_empty_frame_produces_no_blocks() {
        let frame = scene(640, 360);

        assert!(analyse(&frame, Vec::new(), &config()).is_empty());
    }
}
