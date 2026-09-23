//! Collapsing what several sources saw into one list of runs.
//!
//! Two sources looking at the same button report two runs for one label. Translating both would
//! draw the same line twice and pay for it twice, so the merge keeps the better of the two and
//! drops the other.

use crate::{sort_reading_order, TextRun};

/// Two runs count as the same text once this much of their combined area is shared. Half is the
/// point where an overlap is more likely one label seen twice than two labels sitting side by side.
pub const DEFAULT_MIN_IOU: f32 = 0.5;

/// How aggressively [`merge`] de-duplicates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MergePolicy {
    /// Minimum intersection-over-union for two runs to count as the same text.
    pub min_iou: f32,
}

impl Default for MergePolicy {
    fn default() -> Self {
        Self {
            min_iou: DEFAULT_MIN_IOU,
        }
    }
}

/// De-duplicates runs from several sources and returns what is left in reading order.
///
/// The most trustworthy run wins each area: UI Automation over recognition, then the more confident
/// run, then the longer text. A run sharing at least `policy.min_iou` of its area with an already
/// accepted run is dropped, so a label both sources read is translated once and the exact
/// characters from the accessibility tree are the ones that survive.
///
/// Empty runs and zero-area runs are dropped before that ranking. They have no characters to
/// translate and no pixels to draw over, and a zero-area box would survive every overlap test.
pub fn merge(runs: Vec<TextRun>, policy: &MergePolicy) -> Vec<TextRun> {
    let mut ranked = runs;
    ranked.retain(|run| !run.text.trim().is_empty() && !run.bounds.is_empty());
    ranked.sort_by(by_trust);

    let mut kept: Vec<TextRun> = Vec::with_capacity(ranked.len());
    for run in ranked {
        if !kept.iter().any(|other| other.bounds.iou(&run.bounds) >= policy.min_iou) {
            kept.push(run);
        }
    }

    sort_reading_order(&mut kept);
    kept
}

/// Most trustworthy first: the operating system over recognition, then the more confident run, then
/// the longer text. Position breaks the remaining ties, so the result never depends on the order
/// the sources happened to answer in.
fn by_trust(a: &TextRun, b: &TextRun) -> std::cmp::Ordering {
    let rank = b.source.rank().cmp(&a.source.rank());
    let trust = compare_confidence(b.confidence, a.confidence);
    let length = b.text.chars().count().cmp(&a.text.chars().count());
    rank.then(trust).then(length).then(position(a, b))
}

/// Orders two runs by where they are: top to bottom, left to right.
fn position(a: &TextRun, b: &TextRun) -> std::cmp::Ordering {
    a.bounds.y.cmp(&b.bounds.y).then(a.bounds.x.cmp(&b.bounds.x))
}

/// Orders two confidences without letting a `NaN` from a misbehaving engine break the sort.
fn compare_confidence(a: f32, b: f32) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceKind;
    use lumen_core::Rect;

    fn run(text: &str, bounds: Rect, source: SourceKind, confidence: f32) -> TextRun {
        TextRun::new(text, bounds, source, confidence)
    }

    /// A pair whose overlap is exactly half of their combined area.
    fn half_overlapping() -> (Rect, Rect) {
        (Rect::new(0, 0, 10, 10), Rect::new(0, 0, 20, 10))
    }

    fn texts(runs: &[TextRun]) -> Vec<&str> {
        runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[test]
    fn the_operating_system_wins_the_area_both_sources_read() {
        let (small, wide) = half_overlapping();
        let guessed = run("Start", small, SourceKind::Ocr, 0.99);
        let exact = run("Start game", wide, SourceKind::UiAutomation, 1.0);

        let merged = merge(vec![guessed, exact], &MergePolicy::default());

        assert_eq!(texts(&merged), vec!["Start game"]);
        assert_eq!(merged[0].source, SourceKind::UiAutomation);
    }

    #[test]
    fn order_of_arrival_does_not_decide_who_wins() {
        let (small, wide) = half_overlapping();
        let exact = run("exact", small, SourceKind::UiAutomation, 1.0);
        let guessed = run("guessed", wide, SourceKind::Ocr, 0.9);

        let exact_first = vec![exact.clone(), guessed.clone()];
        let guessed_first = vec![guessed, exact];
        let first = merge(exact_first, &MergePolicy::default());
        let second = merge(guessed_first, &MergePolicy::default());

        assert_eq!(texts(&first), vec!["exact"]);
        assert_eq!(texts(&first), texts(&second));
    }

    #[test]
    fn an_overlap_at_exactly_the_threshold_still_merges() {
        let (small, wide) = half_overlapping();
        assert!((small.iou(&wide) - DEFAULT_MIN_IOU).abs() < f32::EPSILON);

        let small_run = run("a", small, SourceKind::Ocr, 0.5);
        let wide_run = run("b", wide, SourceKind::Ocr, 0.5);

        assert_eq!(merge(vec![small_run, wide_run], &MergePolicy::default()).len(), 1);
    }

    #[test]
    fn runs_that_barely_overlap_are_two_labels_not_one() {
        let left = run("left", Rect::new(0, 0, 10, 10), SourceKind::Ocr, 0.9);
        let right = run("right", Rect::new(15, 0, 10, 10), SourceKind::Ocr, 0.9);

        let merged = merge(vec![right, left], &MergePolicy::default());

        assert_eq!(texts(&merged), vec!["left", "right"]);
    }

    #[test]
    fn the_more_confident_recognition_run_wins_a_tie_between_peers() {
        let (small, wide) = half_overlapping();
        let blurry = run("blurry", small, SourceKind::Ocr, 0.4);
        let sharp = run("sharp", wide, SourceKind::Ocr, 0.9);

        let merged = merge(vec![blurry, sharp], &MergePolicy::default());

        assert_eq!(texts(&merged), vec!["sharp"]);
    }

    #[test]
    fn the_longer_text_breaks_a_tie_on_confidence() {
        let (small, wide) = half_overlapping();
        let short = run("Play", small, SourceKind::Ocr, 0.9);
        let long = run("Play now", wide, SourceKind::Ocr, 0.9);

        let merged = merge(vec![short, long], &MergePolicy::default());

        assert_eq!(texts(&merged), vec!["Play now"]);
    }

    #[test]
    fn a_tighter_threshold_keeps_more_runs() {
        let (small, wide) = half_overlapping();
        let strict = MergePolicy { min_iou: 0.8 };

        let small_run = run("a", small, SourceKind::Ocr, 0.9);
        let wide_run = run("b", wide, SourceKind::Ocr, 0.9);

        assert_eq!(texts(&merge(vec![small_run, wide_run], &strict)), vec!["a", "b"]);
    }

    #[test]
    fn survivors_come_back_in_reading_order() {
        let bottom = run("bottom", Rect::new(0, 90, 10, 10), SourceKind::Ocr, 0.9);
        let top_right = run("top-right", Rect::new(60, 0, 10, 10), SourceKind::Ocr, 0.9);
        let top_left = run("top-left", Rect::new(0, 0, 10, 10), SourceKind::UiAutomation, 1.0);

        let merged = merge(vec![bottom, top_right, top_left], &MergePolicy::default());

        assert_eq!(texts(&merged), vec!["top-left", "top-right", "bottom"]);
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(merge(Vec::new(), &MergePolicy::default()).is_empty());
    }

    #[test]
    fn empty_and_zero_area_runs_are_dropped() {
        let blank = run("   ", Rect::new(0, 0, 10, 10), SourceKind::Ocr, 0.9);
        let empty = run("", Rect::new(20, 0, 10, 10), SourceKind::UiAutomation, 1.0);
        let flat = run("flat", Rect::new(40, 0, 10, 0), SourceKind::Ocr, 0.9);
        let kept = run("kept", Rect::new(60, 0, 10, 10), SourceKind::Ocr, 0.8);

        let merged = merge(vec![blank, empty, flat, kept], &MergePolicy::default());

        assert_eq!(texts(&merged), vec!["kept"]);
    }
}
