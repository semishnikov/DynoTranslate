//! Layout analysis.
//!
//! Sources report runs of text with a box around each. That is not yet something a translator can
//! work with: a dialogue box arrives as four separate runs, a button label as one, and nothing says
//! which is which or what colour either sits on. This crate closes that gap. Runs become lines,
//! lines become blocks, every block is classified and its colours are measured, and the result comes
//! back in reading order.
//!
//! Everything here is portable and deterministic, which is what lets the classification be pinned by
//! tests and the compositor be checked against golden images on a build agent.

pub mod blocks;
pub mod colour;
pub mod lines;

pub use blocks::{analyse, group_blocks, Alignment, Block, BlockKind};
pub use colour::{luminance, modal_colour, near, sample_pair, TRANSPARENT};
pub use lines::{group_lines, Line};

/// Thresholds for grouping and classification.
///
/// Every distance is expressed in line heights and every extent as a fraction of the window, so one
/// setting works at any resolution and any interface scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutConfig {
    /// Share of the shorter of two runs' heights they must overlap vertically to share a line.
    pub min_vertical_overlap: f32,
    /// Widest gap between two runs on one line, in line heights.
    pub max_line_gap: f32,
    /// Widest gap between two lines of one block, in line heights.
    pub max_line_spacing: f32,
    /// Share of the narrower of two lines' widths they must overlap to share a block.
    pub min_horizontal_overlap: f32,
    /// How far from the middle or the right edge a block may sit and still count as centred or
    /// right-aligned, as a fraction of the window width.
    pub alignment_tolerance: f32,
    /// Left-edge drift allowed between two entries of one menu, in line heights.
    pub menu_edge_tolerance: f32,
    /// Widest gap between two entries of one menu, in line heights. Deliberately wider than
    /// [`LayoutConfig::max_line_spacing`]: menu entries have padding between them, and if the two
    /// agreed the whole menu would collapse into a single block.
    pub menu_gap: f32,
    /// How many stacked entries make a menu rather than a column of unrelated captions.
    pub menu_stack_minimum: usize,
    /// Longest text a single line may carry and still be a button rather than a label.
    pub button_text_limit: usize,
    /// Share of the window height at the bottom that counts as the subtitle band.
    pub subtitle_band: f32,
    /// Share of the window width a subtitle must cover.
    pub subtitle_min_width: f32,
    /// Share of the window height at the bottom that counts as the dialogue band.
    pub dialogue_band: f32,
    /// Share of the window width a dialogue block must cover.
    pub dialogue_min_width: f32,
    /// Narrower than this share of the window width, a multi-line block is a tooltip.
    pub tooltip_max_width: f32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            min_vertical_overlap: 0.5,
            max_line_gap: 1.5,
            max_line_spacing: 0.9,
            min_horizontal_overlap: 0.4,
            alignment_tolerance: 0.05,
            menu_edge_tolerance: 0.5,
            menu_gap: 2.5,
            menu_stack_minimum: 3,
            button_text_limit: 48,
            subtitle_band: 0.25,
            subtitle_min_width: 0.3,
            dialogue_band: 0.5,
            dialogue_min_width: 0.5,
            tooltip_max_width: 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_gap_is_wider_than_the_line_spacing_so_a_menu_survives_grouping() {
        let config = LayoutConfig::default();

        assert!(config.menu_gap > config.max_line_spacing);
        assert!(config.menu_stack_minimum > 1);
    }

    #[test]
    fn every_threshold_is_a_usable_fraction() {
        let config = LayoutConfig::default();
        let fractions = [
            config.min_vertical_overlap,
            config.max_line_gap,
            config.max_line_spacing,
            config.min_horizontal_overlap,
            config.alignment_tolerance,
            config.menu_edge_tolerance,
            config.menu_gap,
            config.subtitle_band,
            config.subtitle_min_width,
            config.dialogue_band,
            config.dialogue_min_width,
            config.tooltip_max_width,
        ];

        assert!(fractions.iter().all(|value| *value > 0.0));
        assert!(config.min_vertical_overlap <= 1.0);
        assert!(config.min_horizontal_overlap <= 1.0);
    }
}
