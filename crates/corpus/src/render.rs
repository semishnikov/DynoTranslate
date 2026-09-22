//! Drawing text into a frame from the stroke skeletons in [`crate::font`].
//!
//! Every stroke becomes a chain of capsules: for each pixel near a segment the coverage is how
//! much of the stroke radius reaches the pixel centre, and the ink is that fraction of the ink
//! colour over what the frame already held. The same arithmetic in the same order always produces
//! the same bytes, so two runs of the corpus compare equal and a golden image stays meaningful.
//!
//! A line is drawn from its origin and baseline, which is how text on a screen is positioned, and
//! the drawing reports the box its ink actually touched — the tightest ground truth a recognition
//! engine could be measured against, because it is measured from the pixels rather than promised
//! from the metrics.

use lumen_core::{Frame, Rect};

use crate::font::{self, FontStyle, CAP_HEIGHT};
use crate::CorpusError;

/// How far above the baseline the tallest glyph plus its stroke reaches, in design units.
const ASCENT_UNITS: f32 = 20.0;
/// How far below the baseline the deepest descender plus its stroke reaches, in design units.
const DESCENT_UNITS: f32 = 6.0;

/// Everything that decides how one line of text looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Cap height in pixels.
    pub size: f32,
    pub style: FontStyle,
    /// Ink colour as BGRA, the frame's own byte order.
    pub color: [u8; 4],
}

/// How much room a line needs before it is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMeasure {
    pub width: f32,
    /// Space needed above the baseline.
    pub ascent: f32,
    /// Space needed below the baseline.
    pub descent: f32,
}

impl LineMeasure {
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

/// How wide and tall `text` would be drawn, without drawing it. Every character must be covered by
/// the font; layout cannot work with a promise the renderer would later break.
pub fn measure(text: &str, text_style: &TextStyle) -> Result<LineMeasure, CorpusError> {
    let unit = text_style.size / CAP_HEIGHT;
    let mut width = 0.0;
    for character in text.chars() {
        let Some(resolved) = font::glyph(character) else {
            return Err(CorpusError::MissingGlyph { character });
        };
        width += resolved.advance() * unit;
    }
    Ok(LineMeasure {
        width,
        ascent: ASCENT_UNITS * unit,
        descent: DESCENT_UNITS * unit,
    })
}

/// Draws `text` with its left edge at `origin_x` and its baseline at `baseline_y`, and returns the
/// box of the ink it actually laid down. Text that draws nothing — the empty string — returns a
/// zero-width box at the origin.
pub fn draw_line(
    frame: &mut Frame,
    origin_x: f32,
    baseline_y: f32,
    text: &str,
    text_style: &TextStyle,
) -> Result<Rect, CorpusError> {
    let unit = text_style.size / CAP_HEIGHT;
    let slanted = text_style.style.is_slanted();
    let slant = if slanted { font::SLANT } else { 0.0 };
    let mut pen = origin_x;
    let mut ink: Option<Rect> = None;

    for character in text.chars() {
        let Some(resolved) = font::glyph(character) else {
            return Err(CorpusError::MissingGlyph { character });
        };
        let glyph_unit = unit * resolved.scale;
        let radius = text_style.style.stroke_radius() * glyph_unit;
        for stroke in resolved.def.strokes {
            for pair in stroke.windows(2) {
                let [x0, y0] = pair[0];
                let [x1, y1] = pair[1];
                let px0 = pen + (f32::from(x0) + slant * f32::from(y0)) * glyph_unit;
                let py0 = baseline_y - f32::from(y0) * glyph_unit;
                let px1 = pen + (f32::from(x1) + slant * f32::from(y1)) * glyph_unit;
                let py1 = baseline_y - f32::from(y1) * glyph_unit;
                if let Some(segment_ink) = paint_segment(frame, px0, py0, px1, py1, radius, text_style.color) {
                    ink = Some(ink.map_or(segment_ink, |known| known.union(&segment_ink)));
                }
            }
        }
        pen += resolved.advance() * unit;
    }

    let fallback = Rect::new(origin_x.round() as i32, baseline_y.round() as i32, 0, 0);
    Ok(ink.unwrap_or(fallback))
}

/// Blends one capsule into the frame and reports the box of the pixels whose bytes it changed.
/// Measuring the change rather than the coverage keeps the ground-truth box honest: a pixel that
/// rounds back to what it already was did not receive ink, whatever the coverage arithmetic said.
fn paint_segment(frame: &mut Frame, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, color: [u8; 4]) -> Option<Rect> {
    let reach = radius + 0.75;
    let left = ((x0.min(x1) - reach).floor() as i64).max(0) as u32;
    let top = ((y0.min(y1) - reach).floor() as i64).max(0) as u32;
    let right = ((x0.max(x1) + reach).ceil() as i64 + 1).clamp(0, i64::from(frame.width())) as u32;
    let bottom = ((y0.max(y1) + reach).ceil() as i64 + 1).clamp(0, i64::from(frame.height())) as u32;
    if left >= right || top >= bottom {
        return None;
    }

    let mut touched: Option<Rect> = None;
    for y in top..bottom {
        for x in left..right {
            let distance = segment_distance(x as f32 + 0.5, y as f32 + 0.5, x0, y0, x1, y1);
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            if blend(frame, x, y, color, coverage) {
                let pixel = Rect::new(x as i32, y as i32, 1, 1);
                touched = Some(touched.map_or(pixel, |known| known.union(&pixel)));
            }
        }
    }
    touched
}

/// Distance from a point to a line segment.
fn segment_distance(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared < 1e-9 {
        0.0
    } else {
        (((px - x0) * dx + (py - y0) * dy) / length_squared).clamp(0.0, 1.0)
    };
    let cx = x0 + t * dx;
    let cy = y0 + t * dy;
    ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt()
}

/// Puts `coverage` of `color` over the pixel the frame already has, and says whether the pixel's
/// bytes actually changed.
fn blend(frame: &mut Frame, x: u32, y: u32, color: [u8; 4], coverage: f32) -> bool {
    let current = frame.pixel(x, y);
    let blended: [u8; 4] = std::array::from_fn(|channel| {
        (f32::from(color[channel]) * coverage + f32::from(current[channel]) * (1.0 - coverage)).round() as u8
    });
    frame.set_pixel(x, y, blended);
    blended != current
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: [u8; 4] = [235, 232, 228, 255];
    const PAPER: [u8; 4] = [30, 26, 24, 255];

    fn style(style: FontStyle, size: f32) -> TextStyle {
        let color = INK;
        TextStyle { size, style, color }
    }

    fn paper(width: u32, height: u32) -> Frame {
        Frame::filled(width, height, PAPER).unwrap()
    }

    fn ink_pixels(frame: &Frame) -> Vec<(u32, u32)> {
        let mut found = Vec::new();
        for y in 0..frame.height() {
            for x in 0..frame.width() {
                if frame.pixel(x, y) != PAPER {
                    found.push((x, y));
                }
            }
        }
        found
    }

    #[test]
    fn a_stem_pixel_is_pure_ink_and_the_paper_is_untouched() {
        let mut frame = paper(160, 80);
        let text_style = style(FontStyle::Regular, 28.0);
        let box_ = draw_line(&mut frame, 20.0, 60.0, "H", &text_style).unwrap();

        // "H" left stem sits at grid x=4 of 14-unit cap height, so at size 28 the unit is 2 and
        // the stem centre is pixel x = 20 + 4*2 = 28, halfway up the cap band.
        assert_eq!(frame.pixel(28, 46), INK);
        assert_eq!(frame.pixel(120, 20), PAPER);
        assert!(box_.contains(28, 46), "{box_:?}");
        assert!(box_.width > 0 && box_.height > 0);
    }

    #[test]
    fn the_ink_box_is_tight_and_inside_the_frame() {
        let mut frame = paper(320, 120);
        let text_style = style(FontStyle::Bold, 32.0);
        let box_ = draw_line(&mut frame, 12.0, 80.0, "Handgloves", &text_style).unwrap();

        let frame_bounds = frame.bounds();
        assert!(frame_bounds.contains(box_.x, box_.y));
        assert!(frame_bounds.contains(box_.right() - 1, box_.bottom() - 1));

        let ink = ink_pixels(&frame);
        assert!(ink.iter().all(|(x, y)| box_.contains(*x as i32, *y as i32)));
        // Nothing with real coverage lies outside the reported box, and the box is not empty air.
        assert!(!ink.is_empty());
    }

    #[test]
    fn drawing_twice_produces_identical_bytes() {
        let mut left = paper(240, 80);
        let mut right = paper(240, 80);
        let text_style = style(FontStyle::Italic, 24.0);
        draw_line(&mut left, 8.0, 50.0, "Przykład Щёлк", &text_style).unwrap();
        draw_line(&mut right, 8.0, 50.0, "Przykład Щёлк", &text_style).unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn bold_carries_more_ink_than_regular() {
        let mut regular_frame = paper(240, 80);
        let mut bold_frame = paper(240, 80);
        let regular = style(FontStyle::Regular, 24.0);
        let bold = style(FontStyle::Bold, 24.0);
        draw_line(&mut regular_frame, 8.0, 50.0, "Handgloves", &regular).unwrap();
        draw_line(&mut bold_frame, 8.0, 50.0, "Handgloves", &bold).unwrap();
        assert!(ink_pixels(&bold_frame).len() > ink_pixels(&regular_frame).len());
    }

    #[test]
    fn italic_leans_to_the_right() {
        fn centroid_of_top_and_bottom(frame: &Frame) -> (f32, f32) {
            let ink = ink_pixels(frame);
            let top = ink.iter().map(|(_, y)| *y).min().unwrap();
            let bottom = ink.iter().map(|(_, y)| *y).max().unwrap();
            let band = ((bottom - top) / 4).max(1);
            let top_x: Vec<u32> = ink.iter().filter(|(_, y)| *y <= top + band).map(|(x, _)| *x).collect();
            let bottom_x: Vec<u32> = ink.iter().filter(|(_, y)| *y >= bottom - band).map(|(x, _)| *x).collect();
            let mean = |values: &[u32]| values.iter().map(|x| *x as f32).sum::<f32>() / values.len() as f32;
            (mean(&top_x), mean(&bottom_x))
        }

        let mut upright = paper(160, 80);
        draw_line(&mut upright, 20.0, 60.0, "Ill", &style(FontStyle::Regular, 28.0)).unwrap();
        let (upright_top, upright_bottom) = centroid_of_top_and_bottom(&upright);
        assert!((upright_top - upright_bottom).abs() < 1.0);

        let mut leaning = paper(160, 80);
        draw_line(&mut leaning, 20.0, 60.0, "Ill", &style(FontStyle::Italic, 28.0)).unwrap();
        let (leaning_top, leaning_bottom) = centroid_of_top_and_bottom(&leaning);
        assert!(leaning_top > leaning_bottom + 1.5, "{leaning_top} vs {leaning_bottom}");
    }

    #[test]
    fn empty_text_draws_nothing_and_reports_an_empty_box() {
        let mut frame = paper(64, 32);
        let before = frame.clone();
        let box_ = draw_line(&mut frame, 10.0, 20.0, "", &style(FontStyle::Regular, 12.0)).unwrap();
        assert!(box_.is_empty());
        assert_eq!(frame, before);
    }

    #[test]
    fn a_character_the_font_lacks_is_an_error_not_blank_ink() {
        let mut frame = paper(64, 32);
        let error = draw_line(&mut frame, 4.0, 20.0, "漢字", &style(FontStyle::Regular, 12.0)).unwrap_err();
        assert!(matches!(error, CorpusError::MissingGlyph { character: '漢' }));
        let error = measure("hello 世界", &style(FontStyle::Regular, 12.0)).unwrap_err();
        assert!(matches!(error, CorpusError::MissingGlyph { character: '世' }));
    }

    #[test]
    fn measured_widths_grow_with_the_text_and_the_size() {
        let small = measure("Settings", &style(FontStyle::Regular, 16.0)).unwrap();
        let large = measure("Settings", &style(FontStyle::Regular, 32.0)).unwrap();
        let longer = measure("Settings and more", &style(FontStyle::Regular, 16.0)).unwrap();
        assert!((large.width - small.width * 2.0).abs() < 1e-3);
        assert!(longer.width > small.width);
        assert!(small.ascent > 0.0 && small.descent > 0.0);
        assert_eq!(small.height(), small.ascent + small.descent);
    }

    #[test]
    fn clipped_lines_do_not_panic_at_the_edges() {
        // The stroke reaches past every side of a small frame: the cap band starts above the top
        // edge, the origin is left of the frame, the advance runs past the right edge and the
        // baseline sits above a bottom margin the descenders would cross. Clip, never wrap.
        let mut frame = paper(24, 20);
        let bold = style(FontStyle::Bold, 28.0);
        let box_ = draw_line(&mut frame, -20.0, 16.0, "Wide", &bold).unwrap();
        assert!(box_.x >= 0 && box_.y >= 0, "{box_:?}");
        assert!(box_.right() <= 24 && box_.bottom() <= 20, "{box_:?}");
        assert!(box_.width > 0 && box_.height > 0, "{box_:?}");
    }
}
