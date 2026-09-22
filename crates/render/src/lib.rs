//! Rendering for the overlay: fitting, font matching, glyph drawing and erasure.
//!
//! The architecture draws the translation where the original stood: erase the source text by
//! local inpainting, then draw the translation in the closest matching font, weight, colour and
//! alignment, fitted by measurement and never ellipsised. Everything in this crate is portable
//! and deterministic — the same input always produces the same bytes — which is what makes the
//! visual regression suite meaningful on a build agent.
//!
//! Fonts are bundled rather than taken from the operating system. A golden image compared
//! against system-installed faces would depend on which fonts the build agent happens to ship;
//! the DejaVu files under `fonts/` are loaded from memory so Linux, Windows and macOS rasterise
//! the same pixels. Matching therefore picks among the bundled faces by weight and style; there
//! is no family name to match against, because recognition never reports one.

pub mod fit;
pub mod fonts;
pub mod inpaint;
pub mod text;
pub mod writing;

pub use fit::{fit_text, FitError, FittedText, TextSpec};
pub use fonts::{FontLibrary, FontLibraryError};
pub use inpaint::{erase, inpaint};
pub use text::{draw_fitted, FontWeight, ReadyText, Renderer, TextAlign};
pub use writing::{dominant_direction, writing_mode_of, Direction, WritingMode};

/// The smallest share of the original font size a fitted line may use. Fitting shrinks to make
/// the translation fit the original box; going below four fifths of the size makes interface
/// text unreadable, so the floor is a product rule expressed as data.
pub const MIN_FIT_SCALE: f32 = 0.8;
