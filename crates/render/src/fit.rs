//! Fitting a translation into the box the original occupied.
//!
//! Three rules decide every call, and they are product rules rather than heuristics: the text is
//! never ellipsised, it is never drawn below [`MIN_FIT_SCALE`](crate::MIN_FIT_SCALE) of the
//! preferred size, and width is won by word wrapping before size is given up. When the floor is
//! reached the text is drawn anyway — a line that slightly overflows its box is readable, a
//! truncated one is not.

use serde::{Deserialize, Serialize};

use crate::text::{FontWeight, TextAlign};
use crate::writing::WritingMode;
use crate::MIN_FIT_SCALE;

/// Why a fit could not be attempted.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FitError {
    #[error("the target box is {width}x{height}; both dimensions must be positive")]
    EmptyBox { width: u32, height: u32 },
    #[error("preferred font size {size} must be at least 1")]
    ZeroFontSize { size: u32 },
}

/// Everything that decides how one block of text is typeset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpec {
    /// The text to draw, already translated.
    pub text: String,
    /// Width of the box in pixels.
    pub max_width: u32,
    /// Height of the box in pixels.
    pub max_height: u32,
    /// Size the original text was measured at. Fitting starts here and never goes below
    /// 60 % of it.
    pub preferred_size: u32,
    pub weight: FontWeight,
    pub italic: bool,
    pub align: TextAlign,
    pub writing: WritingMode,
}

impl TextSpec {
    pub fn new(text: impl Into<String>, max_width: u32, max_height: u32, preferred_size: u32) -> Self {
        Self {
            text: text.into(),
            max_width,
            max_height,
            preferred_size,
            weight: FontWeight::Regular,
            italic: false,
            align: TextAlign::Left,
            writing: WritingMode::Horizontal,
        }
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn with_writing(mut self, writing: WritingMode) -> Self {
        self.writing = writing;
        self
    }

    pub fn min_size(&self) -> f32 {
        (self.preferred_size as f32 * MIN_FIT_SCALE).max(1.0)
    }
}

/// The size and wrap a fit settled on. The lines themselves are shaped at draw time so the
/// rasteriser and the fitter cannot disagree about breaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FittedText {
    /// Font size in pixels that the draw pass must use.
    pub size: f32,
    /// Line height in pixels at that size.
    pub line_height: f32,
    /// Whether the fit hit the floor and is allowed to overflow the box.
    pub at_floor: bool,
    /// Share of the preferred size actually used, in 0.6..=1.0.
    pub scale: f32,
}

/// Measures `spec` against `measure` and returns the size to draw at.
///
/// `measure(text, size)` must shape `text` at `size` and report the widest line and how many
/// lines the wrap produced. Passing the real shaper in production and a table in tests keeps
/// the algorithm honest without needing a font file for every edge case.
pub fn fit_text<F>(spec: &TextSpec, measure: F) -> Result<FittedText, FitError>
where
    F: FnMut(&str, f32) -> (f32, u32),
{
    if spec.max_width == 0 || spec.max_height == 0 {
        return Err(FitError::EmptyBox {
            width: spec.max_width,
            height: spec.max_height,
        });
    }
    if spec.preferred_size == 0 {
        return Err(FitError::ZeroFontSize {
            size: spec.preferred_size,
        });
    }

    let preferred = spec.preferred_size as f32;
    let floor = spec.min_size();
    let max_width = spec.max_width as f32;
    let max_height = spec.max_height as f32;

    let mut size = preferred;
    let mut measure = measure;
    // Five-percent steps reach the floor from the preferred size in a handful of iterations;
    // the cap is a guard against a non-monotonic measure function looping forever.
    for _ in 0..64 {
        let line_height = (size * 1.2).ceil().max(1.0);
        let (widest, lines) = measure(&spec.text, size);
        let height = lines as f32 * line_height;
        if widest <= max_width && height <= max_height {
            return Ok(fitted(size, line_height, false, preferred));
        }
        if size <= floor + f32::EPSILON {
            break;
        }
        size = (size * 0.95).max(floor);
        if size <= floor {
            size = floor;
        }
    }

    let line_height = (size * 1.2).ceil().max(1.0);
    Ok(fitted(size, line_height, true, preferred))
}

fn fitted(size: f32, line_height: f32, at_floor: bool, preferred: f32) -> FittedText {
    FittedText {
        size,
        line_height,
        at_floor,
        scale: (size / preferred).clamp(MIN_FIT_SCALE, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_annotations() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                let msg = info.to_string().replace('\n', " | ");
                eprintln!("::error title=test-panic::{msg}");
            }));
        });
    }

    /// A stand-in shaper: each character is `size * 0.5` wide, words never break below the
    /// floor, and a newline forces a new line. Enough to drive the algorithm without a font.
    fn fake_measure(text: &str, size: f32) -> (f32, u32) {
        let mut widest = 0.0_f32;
        let mut current = 0.0_f32;
        let mut lines = 1_u32;
        for ch in text.chars() {
            if ch == '\n' {
                widest = widest.max(current);
                current = 0.0;
                lines += 1;
                continue;
            }
            current += size * 0.5;
            widest = widest.max(current);
        }
        (widest, lines)
    }

    #[test]
    fn text_that_already_fits_keeps_the_preferred_size() {
        install_annotations();
        let spec = TextSpec::new("OK", 200, 40, 16);
        let fitted = fit_text(&spec, fake_measure).unwrap();
        assert_eq!(fitted.size, 16.0);
        assert!(!fitted.at_floor);
        assert_eq!(fitted.scale, 1.0);
    }

    #[test]
    fn a_long_line_shrinks_but_never_below_sixty_percent() {
        install_annotations();
        // 40 chars * 0.5 * 16 = 320 wide against a 100-wide box.
        let text = "a".repeat(40);
        let spec = TextSpec::new(text, 100, 400, 16);
        let fitted = fit_text(&spec, fake_measure).unwrap();
        assert!(fitted.size >= 16.0 * 0.6 - 0.001, "{}", fitted.size);
        assert!(fitted.size < 16.0);
    }

    #[test]
    fn an_impossible_box_stops_at_the_floor_instead_of_ellipsising() {
        install_annotations();
        let text = "b".repeat(80);
        let spec = TextSpec::new(text, 20, 20, 32);
        let fitted = fit_text(&spec, fake_measure).unwrap();
        assert!((fitted.size - 32.0 * 0.6).abs() < 0.5, "{}", fitted.size);
        assert!(fitted.at_floor);
        assert!((fitted.scale - 0.6).abs() < 0.01);
    }

    #[test]
    fn multiline_text_that_overflows_height_shrinks() {
        install_annotations();
        let text = "line\nline\nline\nline\nline\nline";
        let spec = TextSpec::new(text, 200, 40, 16);
        let fitted = fit_text(&spec, fake_measure).unwrap();
        assert!(fitted.size < 16.0);
    }

    #[test]
    fn empty_text_keeps_the_preferred_size() {
        install_annotations();
        let spec = TextSpec::new("", 20, 20, 16);
        let fitted = fit_text(&spec, fake_measure).unwrap();
        assert_eq!(fitted.size, 16.0);
        assert!(!fitted.at_floor);
    }

    #[test]
    fn a_single_character_wider_than_the_box_shrinks_toward_the_floor() {
        install_annotations();
        // One character is `size * 0.5` wide. At 16 that is 8, against a 4-wide box.
        let spec = TextSpec::new("W", 4, 40, 16);
        let fitted = fit_text(&spec, fake_measure).unwrap();
        assert!(fitted.size < 16.0);
        assert!(fitted.size >= 16.0 * 0.6 - 0.001, "{}", fitted.size);
        assert!(fitted.at_floor);
    }

    #[test]
    fn an_empty_box_is_an_error() {
        install_annotations();
        let spec = TextSpec::new("x", 0, 10, 16);
        assert_eq!(
            fit_text(&spec, fake_measure),
            Err(FitError::EmptyBox { width: 0, height: 10 })
        );
    }

    #[test]
    fn a_zero_font_size_is_an_error() {
        install_annotations();
        let spec = TextSpec::new("x", 10, 10, 0);
        assert_eq!(fit_text(&spec, fake_measure), Err(FitError::ZeroFontSize { size: 0 }));
    }
}
