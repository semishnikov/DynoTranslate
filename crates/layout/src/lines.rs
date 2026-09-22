//! Collecting recognised runs into the lines they belong to.
//!
//! A source reports one run per fragment it could read. UI Automation usually hands back a whole
//! line at a time; recognition hands back whatever its segmentation produced. Translation needs
//! lines, because a sentence translated in pieces loses its meaning, so fragments that share a
//! baseline and sit close enough to read as one row are joined here and nowhere else.

use crate::LayoutConfig;
use lumen_core::Rect;
use lumen_source::{sort_reading_order, TextRun};
use serde::{Deserialize, Serialize};

/// One row of text and the box that covers it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// The runs that make up the row, left to right.
    pub runs: Vec<TextRun>,
    /// The smallest box holding every run, in frame pixels.
    pub bounds: Rect,
}

impl Line {
    /// A line holding a single run.
    pub fn single(run: TextRun) -> Self {
        let bounds = run.bounds;
        let runs = vec![run];
        Self { runs, bounds }
    }

    /// The row's characters in reading order. Fragments are joined with a space, which restores the
    /// word boundary that recognition segmentation drops.
    pub fn text(&self) -> String {
        let parts: Vec<&str> = self.runs.iter().map(|run| run.text.as_str()).collect();
        parts.join(" ")
    }

    /// Lowest confidence among the runs: a row is only as trustworthy as its weakest fragment.
    pub fn confidence(&self) -> f32 {
        self.runs.iter().map(|run| run.confidence).fold(1.0, f32::min)
    }
}

/// Collects runs into lines, in reading order.
///
/// The result is deterministic for a given input, which is what lets golden images and benchmark
/// reports be compared between runs.
pub fn group_lines(runs: Vec<TextRun>, config: &LayoutConfig) -> Vec<Line> {
    let mut ordered = runs;
    sort_reading_order(&mut ordered);

    let mut lines: Vec<Line> = Vec::with_capacity(ordered.len());
    for run in ordered {
        let bounds = run.bounds;
        match host(&lines, bounds, config) {
            Some(index) => {
                let line = &mut lines[index];
                line.bounds = line.bounds.union(&bounds);
                line.runs.push(run);
            }
            None => lines.push(Line::single(run)),
        }
    }
    lines
}

/// The line a run continues, if there is one. Later lines are tried first: runs arrive sorted by
/// vertical position, so the row a run belongs to is nearly always a recent one.
fn host(lines: &[Line], bounds: Rect, config: &LayoutConfig) -> Option<usize> {
    (0..lines.len())
        .rev()
        .find(|index| same_row(lines[*index].bounds, bounds, config))
}

/// Whether two boxes are part of one row: they overlap vertically by at least
/// [`LayoutConfig::min_vertical_overlap`] of the shorter of the two, and the gap between them is no
/// wider than [`LayoutConfig::max_line_gap`] line heights.
fn same_row(existing: Rect, candidate: Rect, config: &LayoutConfig) -> bool {
    let shorter = existing.height.min(candidate.height);
    if shorter == 0 {
        return false;
    }

    let top = existing.y.max(candidate.y);
    let bottom = existing.bottom().min(candidate.bottom());
    let overlap = (bottom - top).max(0);
    if (overlap as f32) < (shorter as f32) * config.min_vertical_overlap {
        return false;
    }

    let gap = candidate.x - existing.right();
    gap <= 0 || (gap as f32) <= (shorter as f32) * config.max_line_gap
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_source::SourceKind;

    fn config() -> LayoutConfig {
        LayoutConfig::default()
    }

    fn run(text: &str, x: i32, y: i32, width: u32) -> TextRun {
        TextRun::new(text, Rect::new(x, y, width, 12), SourceKind::Ocr, 0.9)
    }

    #[test]
    fn runs_close_together_on_one_row_form_a_single_line() {
        let lines = group_lines(vec![run("Начать", 10, 40, 40), run("игру", 56, 40, 30)], &config());

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "Начать игру");
        assert_eq!(lines[0].bounds, Rect::new(10, 40, 76, 12));
    }

    #[test]
    fn runs_far_apart_on_one_row_stay_separate() {
        let lines = group_lines(vec![run("left", 0, 40, 20), run("right", 300, 40, 20)], &config());

        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn runs_on_different_rows_stay_separate() {
        let lines = group_lines(vec![run("top", 10, 10, 20), run("bottom", 10, 60, 20)], &config());

        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn a_run_that_barely_touches_the_row_above_starts_a_new_line() {
        let lines = group_lines(vec![run("first", 10, 10, 20), run("second", 40, 20, 20)], &config());

        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn a_run_that_overlaps_the_row_above_enough_joins_it() {
        let lines = group_lines(vec![run("first", 10, 10, 20), run("second", 40, 14, 20)], &config());

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "first second");
    }

    #[test]
    fn lines_come_back_in_reading_order_whatever_order_the_runs_arrived_in() {
        let lines = group_lines(vec![run("b", 10, 60, 20), run("a", 10, 10, 20)], &config());

        assert_eq!(lines[0].text(), "a");
        assert_eq!(lines[1].text(), "b");
    }

    #[test]
    fn a_line_is_only_as_confident_as_its_weakest_run() {
        let weak = TextRun::new("weak", Rect::new(10, 10, 20, 12), SourceKind::Ocr, 0.4);
        let strong = run("strong", 40, 10, 20);

        let lines = group_lines(vec![strong, weak], &config());

        assert!((lines[0].confidence() - 0.4).abs() < f32::EPSILON);
    }
}
