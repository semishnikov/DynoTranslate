//! Presents a recognition engine as a text source.
//!
//! The pipeline merges sources, so everything it reads has to arrive as a [`TextRun`]. This
//! adapter is the whole bridge: it hands the engine the pixels and the regions it was given, and
//! labels what comes back as recognition output.

use lumen_ocr::{OcrEngine, Recognition};

use crate::{ReadRequest, SourceError, SourceKind, TextRun, TextSource};

/// An [`OcrEngine`] seen as a [`TextSource`].
pub struct OcrTextSource<E> {
    engine: E,
}

impl<E: OcrEngine> OcrTextSource<E> {
    pub fn new(engine: E) -> Self {
        Self { engine }
    }

    /// The engine behind the adapter, for benchmarks that need to drive it directly.
    pub const fn engine(&self) -> &E {
        &self.engine
    }
}

impl<E: OcrEngine> TextSource for OcrTextSource<E> {
    fn name(&self) -> &str {
        self.engine.name()
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Ocr
    }

    fn read(&mut self, request: &ReadRequest<'_>) -> Result<Vec<TextRun>, SourceError> {
        let lines = self.engine.recognize(request.frame, request.regions)?;
        Ok(lines.into_iter().map(run_from_recognition).collect())
    }
}

fn run_from_recognition(line: Recognition) -> TextRun {
    TextRun {
        text: line.text,
        bounds: line.bounds,
        source: SourceKind::Ocr,
        confidence: line.confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextTarget;
    use lumen_core::{Frame, Rect};
    use lumen_ocr::StubEngine;

    fn line(text: &str, y: i32) -> Recognition {
        Recognition {
            text: text.to_owned(),
            bounds: Rect::new(0, y, 40, 12),
            confidence: 0.8,
        }
    }

    #[test]
    fn recognised_lines_become_recognition_runs() {
        let script = vec![vec![line("Hello", 0), line("world", 14)]];
        let mut source = OcrTextSource::new(StubEngine::new(script));
        let frame = Frame::filled(64, 32, [10, 10, 10, 255]).unwrap();
        let window = TextTarget {
            id: 7,
            bounds: Rect::new(0, 0, 64, 32),
        };
        let request = ReadRequest {
            frame: &frame,
            target: &window,
            regions: &[],
        };

        let runs = source.read(&request).unwrap();

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "Hello");
        assert_eq!(runs[1].bounds, Rect::new(0, 14, 40, 12));
        assert!(runs.iter().all(|run| run.source == SourceKind::Ocr));
        assert!((runs[0].confidence - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn keeps_the_name_of_the_engine_it_wraps() {
        let source = OcrTextSource::new(StubEngine::new(Vec::new()));
        assert_eq!(source.name(), "stub");
        assert_eq!(source.kind(), SourceKind::Ocr);
    }
}
