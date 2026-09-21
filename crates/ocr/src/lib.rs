//! Text recognition behind one trait.
//!
//! The pipeline only ever sees [`OcrEngine`]: a frame and a list of regions go in, recognised lines
//! come out. Real engines (a local model, the Windows OCR runtime) and the deterministic
//! [`StubEngine`](stub::StubEngine) used by tests and benchmarks are interchangeable through it.
//!
//! Around the engine live the text layers of the pipeline. [`source`] turns recognition and UI
//! Automation into text spans and merges the two views of one screen; [`uia`] is the UI Automation
//! source itself (Windows). [`layout`] groups the resulting lines into classified blocks with
//! sampled colours. [`language`] names the language a text is written in from its script alone.
//! [`benchmark`] scores engines by character error rate.

use lumen_core::{Frame, Rect};

pub mod benchmark;
pub mod language;
pub mod layout;
pub mod source;
pub mod stub;
pub mod uia;

pub use language::LanguageId;
pub use source::{merge, OcrSource, SourceError, TextOrigin, TextSource, TextSpan};
pub use stub::StubEngine;

/// One recognised line of text: the characters in reading order, their bounds in frame pixels, and
/// the engine's mean confidence over the line.
#[derive(Debug, Clone, PartialEq)]
pub struct Recognition {
    pub text: String,
    pub bounds: Rect,
    pub confidence: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("no text was available before the deadline")]
    Timeout,
    #[error("engine '{engine}' failed: {detail}")]
    EngineFailed { engine: String, detail: String },
}

/// Recognises the text a frame is already showing.
///
/// Engines read pixels; they never touch window handles or the overlay. What counts as a line and
/// in which order lines are returned is the engine's decision, pinned by the benchmark transcripts.
pub trait OcrEngine {
    /// Stable identifier used in benchmark reports (`"windows-ocr"`, `"stub"`).
    fn name(&self) -> &str;

    /// Recognises one entry per line in reading order. An empty `regions` slice means the whole
    /// frame; engines crop through [`Frame::crop`], so partially visible regions are clipped
    /// rather than rejected.
    fn recognize(&mut self, frame: &Frame, regions: &[Rect]) -> Result<Vec<Recognition>, OcrError>;
}
