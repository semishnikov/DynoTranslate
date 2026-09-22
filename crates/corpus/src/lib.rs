//! The synthetic recognition corpus.
//!
//! Recognition quality is measured against screens, so the corpus manufactures screens: text in the
//! languages the product supports, drawn with the project's own stroke font in regular, bold and
//! italic, over solid, gradient and noisy backgrounds, with the ground truth recorded as the text
//! and the box of every line. The same scenes feed the character error rate gate around
//! [`lumen_ocr::benchmark`] and the scoring of [`lumen_language::identify`].
//!
//! Everything here is deterministic: a spec and a seed produce the same pixels, the same boxes and
//! the same scores on any machine, which is what lets the gate mean the same thing in a local run
//! and in CI. The font is drawn from stroke skeletons defined in [`font`], so no font file, no
//! model and no platform code is involved; the writing systems the skeletons do not cover yet are
//! listed in [`phrases`] and stay out of the corpus until they do.

pub mod background;
pub mod font;
pub mod gate;
pub mod phrases;
pub mod plan;
pub mod render;
pub mod report;
pub mod rng;
pub mod scene;

/// Why the corpus could not be generated or scored.
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    #[error("the text uses a character the corpus font does not draw: '{character}'")]
    MissingGlyph { character: char },
    #[error("a frame could not be created: {0}")]
    Frame(#[from] lumen_core::FrameError),
    #[error("{0}")]
    BadArgument(String),
}
