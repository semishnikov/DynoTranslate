//! Deterministic scripted text source for tests and benchmarks.
//!
//! [`StubSource`] replays one canned answer per call and repeats the last one once the script runs
//! out, so a merge case can pin exactly what a source reported without touching the operating
//! system or recognising a single pixel.

use crate::{within_regions, ReadRequest, SourceError, SourceKind, TextRun, TextSource};

/// Replays a script of canned answers instead of reading the window.
pub struct StubSource {
    name: String,
    kind: SourceKind,
    script: Vec<Vec<TextRun>>,
    calls: usize,
}

impl StubSource {
    /// Replays `script` in order, reporting `kind` as the source of every run. An empty script
    /// reads nothing.
    pub fn new(name: impl Into<String>, kind: SourceKind, script: Vec<Vec<TextRun>>) -> Self {
        Self {
            name: name.into(),
            kind,
            script,
            calls: 0,
        }
    }

    /// An accessibility-tree double, which is what merge tests need most often.
    pub fn ui_automation(script: Vec<Vec<TextRun>>) -> Self {
        Self::new("ui-automation", SourceKind::UiAutomation, script)
    }

    /// A recognition double.
    pub fn ocr(script: Vec<Vec<TextRun>>) -> Self {
        Self::new("ocr-stub", SourceKind::Ocr, script)
    }

    /// How many times [`TextSource::read`] has run.
    pub const fn calls(&self) -> usize {
        self.calls
    }
}

impl TextSource for StubSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> SourceKind {
        self.kind
    }

    fn read(&mut self, request: &ReadRequest<'_>) -> Result<Vec<TextRun>, SourceError> {
        let answer = if self.script.is_empty() {
            Vec::new()
        } else {
            self.script[self.calls.min(self.script.len() - 1)].clone()
        };
        self.calls += 1;
        Ok(within_regions(answer, request.regions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextTarget;
    use lumen_core::{Frame, Rect};

    fn run(text: &str, x: i32, y: i32) -> TextRun {
        TextRun::new(text, Rect::new(x, y, 10, 10), SourceKind::UiAutomation, 1.0)
    }

    fn request(frame: &Frame, target: &TextTarget, regions: &[Rect]) -> ReadRequest<'_> {
        ReadRequest {
            frame,
            target,
            regions,
        }
    }

    fn target() -> TextTarget {
        TextTarget {
            id: 42,
            bounds: Rect::new(0, 0, 200, 200),
        }
    }

    #[test]
    fn replays_the_script_in_order_and_repeats_the_last_answer() {
        let script = vec![vec![run("first", 0, 0)], vec![run("second", 0, 20)]];
        let mut source = StubSource::ui_automation(script);
        let frame = Frame::filled(8, 8, [0, 0, 0, 255]).unwrap();
        let window = target();

        let first = source.read(&request(&frame, &window, &[])).unwrap();
        let second = source.read(&request(&frame, &window, &[])).unwrap();
        let third = source.read(&request(&frame, &window, &[])).unwrap();

        assert_eq!(first, vec![run("first", 0, 0)]);
        assert_eq!(second, vec![run("second", 0, 20)]);
        assert_eq!(third, vec![run("second", 0, 20)]);
        assert_eq!(source.calls(), 3);
    }

    #[test]
    fn empty_script_reads_nothing() {
        let mut source = StubSource::ocr(Vec::new());
        let frame = Frame::filled(8, 8, [0, 0, 0, 255]).unwrap();
        let window = target();
        assert!(source.read(&request(&frame, &window, &[])).unwrap().is_empty());
    }

    #[test]
    fn reports_its_name_and_kind() {
        let source = StubSource::ui_automation(Vec::new());
        assert_eq!(source.name(), "ui-automation");
        assert_eq!(source.kind(), SourceKind::UiAutomation);
        assert_eq!(StubSource::ocr(Vec::new()).kind(), SourceKind::Ocr);
    }

    #[test]
    fn honours_the_regions_it_is_asked_about() {
        let one_pass = vec![run("visible", 10, 10), run("elsewhere", 900, 900)];
        let mut source = StubSource::ui_automation(vec![one_pass]);
        let frame = Frame::filled(8, 8, [0, 0, 0, 255]).unwrap();
        let window = target();
        let regions = vec![Rect::new(0, 0, 100, 100)];

        let runs = source.read(&request(&frame, &window, &regions)).unwrap();

        assert_eq!(runs, vec![run("visible", 10, 10)]);
    }
}
