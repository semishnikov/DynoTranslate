//! Deterministic scripted engine for tests and benchmarks.
//!
//! [`StubEngine`] replays one canned answer per call and repeats the last one once the script runs
//! out, so a benchmark case can pin the exact hypothesis it scores without rendering any pixels.

use lumen_core::{Frame, Rect};

use crate::{OcrEngine, OcrError, Recognition};

/// Replays a script of canned answers instead of reading pixels.
pub struct StubEngine {
    script: Vec<Vec<Recognition>>,
    calls: usize,
}

impl StubEngine {
    /// Replays `script` in order; an empty script recognises nothing.
    pub fn new(script: Vec<Vec<Recognition>>) -> Self {
        Self { script, calls: 0 }
    }

    /// How many times [`OcrEngine::recognize`] has run.
    pub const fn calls(&self) -> usize {
        self.calls
    }
}

impl OcrEngine for StubEngine {
    fn name(&self) -> &str {
        "stub"
    }

    fn recognize(&mut self, _frame: &Frame, _regions: &[Rect]) -> Result<Vec<Recognition>, OcrError> {
        let answer = if self.script.is_empty() {
            Vec::new()
        } else {
            self.script[self.calls.min(self.script.len() - 1)].clone()
        };
        self.calls += 1;
        Ok(answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> Recognition {
        Recognition {
            text: text.to_owned(),
            bounds: Rect::new(0, 0, 8, 8),
            confidence: 1.0,
        }
    }

    fn test_frame() -> Frame {
        Frame::filled(8, 8, [0, 0, 0, 255]).unwrap()
    }

    #[test]
    fn replays_the_script_in_order_and_repeats_the_last_answer() {
        let mut engine = StubEngine::new(vec![vec![line("first")], vec![line("second")]]);
        let frame = test_frame();
        assert_eq!(engine.recognize(&frame, &[]).unwrap(), vec![line("first")]);
        assert_eq!(engine.recognize(&frame, &[]).unwrap(), vec![line("second")]);
        assert_eq!(engine.recognize(&frame, &[]).unwrap(), vec![line("second")]);
        assert_eq!(engine.calls(), 3);
    }

    #[test]
    fn empty_script_recognises_nothing() {
        let mut engine = StubEngine::new(Vec::new());
        let frame = test_frame();
        assert!(engine.recognize(&frame, &[]).unwrap().is_empty());
    }
}
