//! Shaping and drawing fitted text into a frame.
//!
//! The renderer owns one font system and one glyph cache and draws by rasterising each glyph
//! through swash, then blending it source-over into the frame's straight BGRA pixels. Outline
//! text is drawn as eight one-pixel offsets of the same mask under the fill, which is enough
//! to keep light-on-dark game text readable without a second rasterisation path.

use std::collections::HashSet;

use cosmic_text::{
    Align, Attrs, Buffer, Ellipsize, Family, Metrics, PhysicalGlyph, Shaping, Style, SwashCache, Weight, Wrap,
};
use lumen_core::{Frame, Rect};
use serde::{Deserialize, Serialize};

use crate::fit::{fit_text, FitError, FittedText, TextSpec};
use crate::fonts::{FontLibrary, FontLibraryError};
use crate::writing::{dominant_direction, Direction, WritingMode};

/// Stroke weight the layout measured and the matcher must honour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    #[default]
    Regular,
    Bold,
}

/// Horizontal placement of each line inside the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A fitted block ready to be drawn, produced by [`Renderer::fit`].
#[derive(Debug, Clone, PartialEq)]
pub struct ReadyText {
    pub spec: TextSpec,
    pub fitted: FittedText,
    /// Box the draw pass will actually ink. When the fit is at the floor the glyphs may run
    /// slightly past the target; damage tracking uses this so overflow is still presented.
    pub ink_bounds: Rect,
}

/// Shapes, fits and rasterises text with the bundled faces.
pub struct Renderer {
    library: FontLibrary,
    cache: SwashCache,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("faces", &self.library.face_count())
            .finish()
    }
}

impl Renderer {
    pub fn new() -> Result<Self, FontLibraryError> {
        Ok(Self {
            library: FontLibrary::bundled()?,
            cache: SwashCache::new(),
        })
    }

    /// Fits `spec` into its box and returns what [`Renderer::draw`] will ink.
    pub fn fit(&mut self, spec: &TextSpec) -> Result<ReadyText, FitError> {
        let fitted = fit_text(spec, |text, size| self.measure(text, size, spec))?;
        let ink_bounds = self.ink_bounds(spec, &fitted);
        Ok(ReadyText {
            spec: spec.clone(),
            fitted,
            ink_bounds,
        })
    }

    /// Draws `ready` with its top-left corner at `origin` inside `frame`.
    ///
    /// `opacity` scales every alpha the pass writes, so a dimmed overlay dims its text with
    /// its plates. Returns the union of the pixels the pass actually touched, in frame
    /// coordinates.
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        ready: &ReadyText,
        origin: (i32, i32),
        foreground: [u8; 4],
        outline: Option<[u8; 4]>,
        opacity: f32,
    ) -> Rect {
        let fill = scale_alpha(foreground, opacity);
        let halo = outline.map(|colour| scale_alpha(colour, opacity));
        self.paint(frame, ready, origin.0, origin.1, fill, halo)
    }

    fn paint(
        &mut self,
        frame: &mut Frame,
        ready: &ReadyText,
        origin_x: i32,
        origin_y: i32,
        fill: [u8; 4],
        outline: Option<[u8; 4]>,
    ) -> Rect {
        let spec = &ready.spec;
        let size = ready.fitted.size;
        let line_height = ready.fitted.line_height;
        let vertical = matches!(spec.writing, WritingMode::Vertical);
        let text = match spec.writing {
            WritingMode::Horizontal => spec.text.clone(),
            WritingMode::Vertical => vertical_text(&spec.text),
        };
        let wrap_width = match spec.writing {
            WritingMode::Horizontal => Some(spec.max_width as f32),
            WritingMode::Vertical => Some((size * 1.35).ceil().max(1.0)),
        };
        let align = match (spec.align, spec.writing) {
            (_, WritingMode::Vertical) => Align::Right,
            (TextAlign::Left, _) => Align::Left,
            (TextAlign::Center, _) => Align::Center,
            (TextAlign::Right, _) => Align::Right,
        };
        let attrs = self.attrs(spec);
        let metrics = Metrics::new(size, line_height);
        let max_width = spec.max_width as f32;
        let max_height = spec.max_height as f32;
        let column_width = (size * 1.35).ceil().max(1.0);
        let per_column = (max_height / line_height).floor().max(1.0) as usize;

        let font_system = self.library.system();
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_size(wrap_width, Some(max_height));
        buffer.set_wrap(Wrap::WordOrGlyph);
        buffer.set_ellipsize(Ellipsize::None);
        buffer.set_text(&text, &attrs, Shaping::Advanced, Some(align));
        buffer.shape_until_scroll(font_system, false);

        // Collect the physical glyphs first: the cache and the frame are both borrowed by the
        // stamping pass, and collecting ends the buffer borrow before drawing begins.
        let mut stamps: Vec<(PhysicalGlyph, [u8; 4], i32, i32)> = Vec::new();
        {
            for run in buffer.layout_runs() {
                let column = if vertical { run.line_i / per_column.max(1) } else { 0 };
                let column_x = if vertical {
                    (max_width - (column as f32 + 1.0) * column_width).max(0.0)
                } else {
                    0.0
                };
                for glyph in run.glyphs.iter() {
                    let physical = glyph.physical((column_x, origin_y as f32 + run.line_y), 1.0);
                    if let Some(halo) = outline {
                        for (dx, dy) in OUTLINE_OFFSETS {
                            stamps.push((shift(&physical, dx, dy), halo, origin_x, origin_y));
                        }
                    }
                    stamps.push((physical, fill, origin_x, origin_y));
                }
            }
        }

        let mut touched: Option<Rect> = None;
        let font_system = self.library.system();
        for (physical, color, ox, oy) in stamps {
            let cache = &mut self.cache;
            let frame = &mut *frame;
            let mut local: Option<Rect> = None;
            cache.with_pixels(font_system, physical.cache_key, cosmic_color(color), |x, y, pixel| {
                let px = physical.x + x + ox;
                let py = physical.y + y + oy;
                if px < 0 || py < 0 {
                    return;
                }
                let (ux, uy) = (px as u32, py as u32);
                if ux >= frame.width() || uy >= frame.height() {
                    return;
                }
                let base = frame.pixel(ux, uy);
                frame.set_pixel(ux, uy, blend(base, mask_colour(pixel, color)));
                let rect = Rect::new(px, py, 1, 1);
                local = Some(match local {
                    Some(previous) => previous.union(&rect),
                    None => rect,
                });
            });
            if let Some(rect) = local {
                touched = Some(match touched {
                    Some(previous) => previous.union(&rect),
                    None => rect,
                });
            }
        }

        touched.unwrap_or(Rect::new(origin_x, origin_y, 0, 0))
    }

    fn attrs(&self, spec: &TextSpec) -> Attrs<'static> {
        let weight = match spec.weight {
            FontWeight::Regular => Weight::NORMAL,
            FontWeight::Bold => Weight::BOLD,
        };
        let style = if spec.italic { Style::Italic } else { Style::Normal };
        Attrs::new().family(Family::SansSerif).weight(weight).style(style)
    }

    fn measure(&mut self, text: &str, size: f32, spec: &TextSpec) -> (f32, u32) {
        let text = match spec.writing {
            WritingMode::Horizontal => text.to_owned(),
            WritingMode::Vertical => vertical_text(text),
        };
        let line_height = (size * 1.2).ceil().max(1.0);
        let wrap_width = match spec.writing {
            WritingMode::Horizontal => spec.max_width as f32,
            WritingMode::Vertical => (size * 1.35).ceil().max(1.0),
        };
        let attrs = self.attrs(spec);
        let font_system = self.library.system();
        let mut buffer = Buffer::new(font_system, Metrics::new(size, line_height));
        buffer.set_size(Some(wrap_width), Some(spec.max_height as f32));
        buffer.set_wrap(Wrap::WordOrGlyph);
        buffer.set_ellipsize(Ellipsize::None);
        buffer.set_text(&text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(font_system, false);

        let mut widest = 0.0_f32;
        let mut lines = 0_u32;
        let mut seen = HashSet::new();
        for run in buffer.layout_runs() {
            widest = widest.max(run.line_w);
            if seen.insert(run.line_i) {
                lines += 1;
            }
        }
        if lines == 0 {
            lines = 1;
        }
        (widest, lines)
    }

    fn ink_bounds(&mut self, spec: &TextSpec, fitted: &FittedText) -> Rect {
        let text = match spec.writing {
            WritingMode::Horizontal => spec.text.clone(),
            WritingMode::Vertical => vertical_text(&spec.text),
        };
        let attrs = self.attrs(spec);
        let wrap_width = match spec.writing {
            WritingMode::Horizontal => Some(spec.max_width as f32),
            WritingMode::Vertical => Some((fitted.size * 1.35).ceil().max(1.0)),
        };
        let font_system = self.library.system();
        let mut buffer = Buffer::new(font_system, Metrics::new(fitted.size, fitted.line_height));
        buffer.set_size(wrap_width, Some(spec.max_height as f32));
        buffer.set_wrap(Wrap::WordOrGlyph);
        buffer.set_ellipsize(Ellipsize::None);
        buffer.set_text(&text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(font_system, false);

        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        let mut any = false;
        let cache = &mut self.cache;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0.0, run.line_y), 1.0);
                let image = cache.get_image(font_system, physical.cache_key);
                if let Some(image) = image {
                    let left = physical.x as f32 + image.placement.left as f32;
                    let top = physical.y as f32 - image.placement.top as f32;
                    min_x = min_x.min(left);
                    max_x = max_x.max(left + image.placement.width as f32);
                    min_y = min_y.min(top);
                    max_y = max_y.max(top + image.placement.height as f32);
                    any = true;
                }
            }
        }
        if !any {
            return Rect::new(0, 0, spec.max_width, spec.max_height);
        }
        let x = min_x.floor().min(0.0) as i32;
        let y = min_y.floor().min(0.0) as i32;
        let right = max_x.ceil().max(spec.max_width as f32) as i32;
        let bottom = max_y.ceil().max(0.0) as i32;
        Rect::new(x, y, (right - x).max(0) as u32, (bottom - y).max(1) as u32)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new().expect("bundled fonts are valid")
    }
}

const OUTLINE_OFFSETS: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

fn shift(physical: &PhysicalGlyph, dx: i32, dy: i32) -> PhysicalGlyph {
    PhysicalGlyph {
        cache_key: physical.cache_key,
        x: physical.x + dx,
        y: physical.y + dy,
    }
}

fn cosmic_color(rgba: [u8; 4]) -> cosmic_text::Color {
    // Frame bytes are BGRA; cosmic-text wants RGBA.
    cosmic_text::Color::rgba(rgba[2], rgba[1], rgba[0], rgba[3])
}

/// Rebuilds a straight-alpha pixel from what `with_pixels` reported.
///
/// For a mask glyph the callback's colour keeps the base RGB and puts the coverage in alpha.
fn mask_colour(pixel: cosmic_text::Color, base: [u8; 4]) -> [u8; 4] {
    let coverage = pixel.a();
    if coverage == 0 {
        return [0, 0, 0, 0];
    }
    [base[0], base[1], base[2], coverage]
}

fn blend(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 {
        return dst;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mut out = [0_u8; 4];
    for channel in 0..3 {
        let sc = src[channel] as f32;
        let dc = dst[channel] as f32;
        let value = (sc * sa + dc * da * (1.0 - sa)) / out_a;
        out[channel] = value.round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    out
}

fn scale_alpha(colour: [u8; 4], opacity: f32) -> [u8; 4] {
    let opacity = opacity.clamp(0.0, 1.0);
    let mut out = colour;
    out[3] = (colour[3] as f32 * opacity).round().clamp(0.0, 255.0) as u8;
    out
}

fn _direction_hint(text: &str) -> Direction {
    dominant_direction(text)
}

/// One character per line, which stacks them top to bottom for vertical setting.
fn vertical_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for (i, ch) in text.chars().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if ch == ' ' {
            // A space is a blank cell rather than a line break.
            out.push('\u{00A0}');
        } else {
            out.push(ch);
        }
    }
    out
}

/// Fits and draws in one call, which is what the compositor uses per block.
pub fn draw_fitted(
    renderer: &mut Renderer,
    frame: &mut Frame,
    spec: &TextSpec,
    origin: (i32, i32),
    foreground: [u8; 4],
    outline: Option<[u8; 4]>,
    opacity: f32,
) -> Result<Rect, FitError> {
    let ready = renderer.fit(spec)?;
    Ok(renderer.draw(frame, &ready, origin, foreground, outline, opacity))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> Renderer {
        Renderer::new().expect("bundled fonts")
    }

    fn ink_pixels(frame: &Frame) -> usize {
        frame.as_bytes().chunks_exact(4).filter(|pixel| pixel[3] > 0).count()
    }

    #[test]
    fn drawing_text_puts_ink_in_the_box() {
        let mut renderer = renderer();
        let mut frame = Frame::filled(200, 60, [0, 0, 0, 0]).expect("dims");
        let spec = TextSpec::new("Continue", 180, 40, 18);
        let ready = renderer.fit(&spec).expect("fit");
        renderer.draw(&mut frame, &ready, (10, 10), [255, 255, 255, 255], None, 1.0);
        assert!(ink_pixels(&frame) > 20, "expected visible glyphs");
    }

    #[test]
    fn the_same_input_produces_the_same_bytes() {
        let draw = || {
            let mut renderer = renderer();
            let mut frame = Frame::filled(160, 48, [0, 0, 0, 0]).expect("dims");
            let spec = TextSpec::new("Настройки", 140, 32, 16).with_weight(FontWeight::Bold);
            let ready = renderer.fit(&spec).expect("fit");
            renderer.draw(&mut frame, &ready, (4, 8), [240, 240, 240, 255], None, 1.0);
            frame.as_bytes().to_vec()
        };
        assert_eq!(draw(), draw());
    }

    #[test]
    fn fitting_never_reports_below_the_floor() {
        let mut renderer = renderer();
        let spec = TextSpec::new(
            "This sentence is far too long for the tiny box it has been given to live in",
            80,
            24,
            24,
        );
        let ready = renderer.fit(&spec).expect("fit");
        assert!(ready.fitted.size >= 24.0 * 0.8 - 0.01, "{}", ready.fitted.size);
    }

    #[test]
    fn drawing_does_not_ellipsise() {
        let mut renderer = renderer();
        let text = "Extremely long label that cannot possibly fit";
        let mut frame = Frame::filled(60, 40, [0, 0, 0, 0]).expect("dims");
        let spec = TextSpec::new(text, 50, 20, 20);
        draw_fitted(
            &mut renderer,
            &mut frame,
            &spec,
            (0, 0),
            [255, 255, 255, 255],
            None,
            1.0,
        )
        .expect("fit");
        assert!(!spec.text.contains('…'));
        assert!(ink_pixels(&frame) > 0);
    }

    #[test]
    fn bold_draws_more_ink_than_regular() {
        let measure = |weight: FontWeight| {
            let mut renderer = renderer();
            let mut frame = Frame::filled(220, 48, [0, 0, 0, 0]).expect("dims");
            let spec = TextSpec::new("AGGRESSIVE", 200, 40, 28).with_weight(weight);
            let ready = renderer.fit(&spec).expect("fit");
            renderer.draw(&mut frame, &ready, (4, 4), [255, 255, 255, 255], None, 1.0);
            ink_pixels(&frame)
        };
        let regular = measure(FontWeight::Regular);
        let bold = measure(FontWeight::Bold);
        assert!(bold > regular, "bold {bold} should exceed regular {regular}");
    }

    #[test]
    fn right_alignment_shifts_ink_to_the_right() {
        let mut renderer = renderer();
        let mut left = Frame::filled(200, 40, [0, 0, 0, 0]).expect("dims");
        let mut right = Frame::filled(200, 40, [0, 0, 0, 0]).expect("dims");
        let base = TextSpec::new("End", 180, 30, 18);
        let left_ready = renderer.fit(&base).expect("fit");
        renderer.draw(&mut left, &left_ready, (0, 4), [255, 255, 255, 255], None, 1.0);
        let right_spec = base.clone().with_align(TextAlign::Right);
        let right_ready = renderer.fit(&right_spec).expect("fit");
        renderer.draw(&mut right, &right_ready, (0, 4), [255, 255, 255, 255], None, 1.0);

        let centroid = |frame: &Frame| {
            let mut sum = 0_u32;
            let mut count = 0_u32;
            for y in 0..frame.height() {
                for x in 0..frame.width() {
                    if frame.pixel(x, y)[3] > 0 {
                        sum += x;
                        count += 1;
                    }
                }
            }
            sum as f32 / count as f32
        };
        assert!(
            centroid(&right) > centroid(&left) + 10.0,
            "right {} should beat left {}",
            centroid(&right),
            centroid(&left)
        );
    }

    #[test]
    fn vertical_fitting_keeps_the_floor() {
        let mut renderer = renderer();
        let spec = TextSpec::new("メニューを開く", 40, 150, 20).with_writing(WritingMode::Vertical);
        let ready = renderer.fit(&spec).expect("fit");
        assert!(ready.fitted.size >= 20.0 * 0.8 - 0.01);
    }

    #[test]
    fn an_outline_pass_runs_alongside_the_fill() {
        let mut renderer = renderer();
        let mut without = Frame::filled(200, 48, [0, 0, 0, 0]).expect("dims");
        let mut with = Frame::filled(200, 48, [0, 0, 0, 0]).expect("dims");
        let spec = TextSpec::new("Outlined", 180, 36, 20);
        let ready = renderer.fit(&spec).expect("fit");
        renderer.draw(&mut without, &ready, (8, 8), [255, 255, 255, 255], None, 1.0);
        renderer.draw(
            &mut with,
            &ready,
            (8, 8),
            [255, 255, 255, 255],
            Some([0, 0, 0, 255]),
            1.0,
        );
        assert!(ink_pixels(&with) > ink_pixels(&without));
    }

    #[test]
    fn opacity_scales_the_ink() {
        let mut renderer = renderer();
        let mut opaque = Frame::filled(200, 48, [0, 0, 0, 0]).expect("dims");
        let mut faded = Frame::filled(200, 48, [0, 0, 0, 0]).expect("dims");
        let spec = TextSpec::new("Dim", 180, 36, 20);
        let ready = renderer.fit(&spec).expect("fit");
        renderer.draw(&mut opaque, &ready, (4, 4), [255, 255, 255, 255], None, 1.0);
        renderer.draw(&mut faded, &ready, (4, 4), [255, 255, 255, 255], None, 0.5);
        let total = |frame: &Frame| -> u32 { frame.as_bytes().chunks_exact(4).map(|p| u32::from(p[3])).sum() };
        assert!(total(&faded) < total(&opaque));
    }

    #[test]
    fn hebrew_shapes_without_panicking() {
        let mut renderer = renderer();
        let mut frame = Frame::filled(160, 48, [0, 0, 0, 0]).expect("dims");
        let spec = TextSpec::new("שלום", 140, 36, 20);
        let ready = renderer.fit(&spec).expect("fit");
        renderer.draw(&mut frame, &ready, (4, 4), [255, 255, 255, 255], None, 1.0);
        assert!(ink_pixels(&frame) > 0, "Hebrew must produce glyphs");
    }
}
