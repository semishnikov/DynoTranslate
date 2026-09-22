//! Text sources.
//!
//! A text source answers one question: what text is this window showing right now, and where? The
//! pipeline only ever sees [`TextSource`], so the operating-system-backed `UiAutomationSource`, an
//! [`OcrEngine`](lumen_ocr::OcrEngine) wrapped by [`OcrTextSource`], and the scripted
//! [`StubSource`] used in tests are interchangeable.
//!
//! Sources overlap by design: UI Automation reports the same label recognition just read off the
//! pixels. [`merge`] resolves that with one rule set pinned by tests, so a block is translated once
//! no matter how many sources saw it.

use lumen_core::{Frame, Rect};
use serde::{Deserialize, Serialize};

pub mod merge;
pub mod ocr;
pub mod stub;

#[cfg(windows)]
pub mod windows;

pub use merge::{merge, MergePolicy, DEFAULT_MIN_IOU};
pub use ocr::OcrTextSource;
pub use stub::StubSource;

/// How a run was obtained, which is what the merge ranks by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    /// Read from the operating system's accessibility tree: the exact characters the application
    /// says it drew, in the exact rectangle it drew them.
    UiAutomation,
    /// Read from pixels by a recognition engine: right most of the time, and the only source for
    /// games, video and anything else that draws text as an image.
    Ocr,
}

impl SourceKind {
    /// Higher is more trustworthy. The accessibility tree reports what the application drew; a
    /// recognition engine reports what a model believes it saw.
    pub const fn rank(self) -> u8 {
        match self {
            SourceKind::UiAutomation => 1,
            SourceKind::Ocr => 0,
        }
    }
}

/// Identifies the window a source reads. Window handles never leave the platform adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextTarget {
    pub id: u64,
    /// Where the window sits on the desktop, in the units the operating system reports. Sources
    /// convert what they find into frame pixels against this origin.
    pub bounds: Rect,
}

/// Everything a source needs for one pass.
#[derive(Clone, Copy)]
pub struct ReadRequest<'a> {
    /// The captured pixels of the window. Recognition sources read these; operating-system sources
    /// ignore them.
    pub frame: &'a Frame,
    /// The window being read. Operating-system sources walk its accessibility tree; recognition
    /// sources ignore it.
    pub target: &'a TextTarget,
    /// Regions worth reading, in frame pixels. Empty means the whole frame.
    pub regions: &'a [Rect],
}

/// One run of text a source found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    /// The characters, in reading order.
    pub text: String,
    /// Where they are, in frame pixels, clipped to the frame.
    pub bounds: Rect,
    pub source: SourceKind,
    /// How much the source trusts this run, 0.0 to 1.0. Operating-system sources report 1.0: the
    /// application told them the text.
    pub confidence: f32,
}

impl TextRun {
    pub fn new(text: impl Into<String>, bounds: Rect, source: SourceKind, confidence: f32) -> Self {
        Self {
            text: text.into(),
            bounds,
            source,
            confidence,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("the target window is gone or exposes no accessibility tree")]
    TargetLost,
    #[error("{operation} failed: {detail}")]
    Platform { operation: &'static str, detail: String },
    #[error(transparent)]
    Recognition(#[from] lumen_ocr::OcrError),
}

/// Reads the text a window is showing.
///
/// Sources read; they never decide what to translate or where to draw it. Bounds come back in frame
/// pixels, so runs from different sources land on the same grid and can be compared.
pub trait TextSource {
    /// Stable identifier used in reports and merge logs (`"ui-automation"`, `"stub"`).
    fn name(&self) -> &str;

    /// Which kind of source this is, which is what [`merge`] ranks by.
    fn kind(&self) -> SourceKind;

    /// One run per line in reading order, restricted to `request.regions` when it is not empty.
    fn read(&mut self, request: &ReadRequest<'_>) -> Result<Vec<TextRun>, SourceError>;
}

/// Sorts runs into reading order: top to bottom, left to right within one line.
///
/// This is deliberately coarse. Real reading order needs block detection, which lands with layout
/// analysis; what matters here is that the same input always produces the same output, so reports
/// and golden images stay comparable.
pub fn sort_reading_order(runs: &mut [TextRun]) {
    runs.sort_by(|a, b| a.bounds.y.cmp(&b.bounds.y).then(a.bounds.x.cmp(&b.bounds.x)));
}

/// Keeps the runs that fall inside `regions`.
///
/// An empty region list keeps everything: no change detection ran, so the whole frame is fair game.
pub fn within_regions(runs: Vec<TextRun>, regions: &[Rect]) -> Vec<TextRun> {
    if regions.is_empty() {
        return runs;
    }
    runs.into_iter()
        .filter(|run| regions.iter().any(|region| region.intersects(&run.bounds)))
        .collect()
}

/// Converts a rectangle in desktop coordinates into frame pixels, clipping it to the frame.
///
/// Runs the window has scrolled or clipped away come back as `None`: translating text that is not
/// on screen would put a line where the user cannot read it.
pub fn local_bounds(desktop: Rect, frame: Rect) -> Option<Rect> {
    let x = desktop.x - frame.x;
    let y = desktop.y - frame.y;
    let shifted = Rect::new(x, y, desktop.width, desktop.height);
    let frame_local = Rect::new(0, 0, frame.width, frame.height);
    shifted.intersection(&frame_local)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, x: i32, y: i32) -> TextRun {
        let bounds = Rect::new(x, y, 10, 10);
        TextRun::new(text, bounds, SourceKind::UiAutomation, 1.0)
    }

    fn texts(runs: &[TextRun]) -> Vec<&str> {
        runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[test]
    fn operating_system_source_outranks_recognition() {
        assert!(SourceKind::UiAutomation.rank() > SourceKind::Ocr.rank());
    }

    #[test]
    fn reading_order_sorts_top_to_bottom_then_left_to_right() {
        let mut runs = vec![run("c", 40, 20), run("a", 5, 5), run("b", 30, 5), run("d", 0, 20)];

        sort_reading_order(&mut runs);

        assert_eq!(texts(&runs), vec!["a", "b", "d", "c"]);
    }

    #[test]
    fn no_regions_keeps_every_run() {
        let runs = vec![run("a", 0, 0), run("b", 500, 500)];
        assert_eq!(within_regions(runs, &[]).len(), 2);
    }

    #[test]
    fn regions_keep_only_the_runs_they_touch() {
        let regions = vec![Rect::new(0, 0, 100, 100)];
        let runs = vec![run("inside", 10, 10), run("outside", 500, 500)];

        let kept = within_regions(runs, &regions);

        assert_eq!(texts(&kept), vec!["inside"]);
    }

    #[test]
    fn desktop_coordinates_become_frame_pixels() {
        let frame = Rect::new(100, 200, 800, 600);
        let label = Rect::new(150, 260, 40, 20);
        let expected = Some(Rect::new(50, 60, 40, 20));

        assert_eq!(local_bounds(label, frame), expected);
    }

    #[test]
    fn a_partly_visible_run_is_clipped_to_the_frame() {
        let frame = Rect::new(0, 0, 100, 100);
        let label = Rect::new(80, 10, 40, 20);
        let expected = Some(Rect::new(80, 10, 20, 20));

        assert_eq!(local_bounds(label, frame), expected);
    }

    #[test]
    fn a_run_outside_the_frame_is_dropped() {
        let frame = Rect::new(0, 0, 100, 100);

        assert_eq!(local_bounds(Rect::new(400, 10, 40, 20), frame), None);
        assert_eq!(local_bounds(Rect::new(-60, 10, 40, 20), frame), None);
    }
}
