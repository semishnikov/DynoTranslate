use lumen_core::{coalesce, Frame, Rect};
use lumen_render::{erase, FontWeight, Renderer, TextAlign, TextSpec, WritingMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverlayStyle {
    /// Erase the original text and draw the translation in its place.
    Seamless,
    /// Draw the translation on an opaque plate, for backgrounds too complex to erase convincingly.
    Plate,
    /// Collect the translation into a band at the bottom of the frame.
    Subtitles,
}

/// One recognised region and the text that replaces it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayBlock {
    pub rect: Rect,
    pub text: String,
    pub background: [u8; 4],
    pub foreground: [u8; 4],
    /// Optional stroke colour drawn under the glyphs, for light-on-dark game text.
    pub outline: Option<[u8; 4]>,
    /// Recognition confidence in 0..=1. Low-confidence blocks fall back to a plate because erasing
    /// text that was read badly is worse than covering it.
    pub confidence: f32,
    /// Glyph height the layout measured. Fitting starts here and never drops below 80 % of it.
    pub font_size: u32,
    pub weight: FontWeight,
    pub italic: bool,
    pub align: TextAlign,
    pub writing: WritingMode,
}

impl OverlayBlock {
    pub fn new(rect: Rect, text: impl Into<String>) -> Self {
        Self {
            rect,
            text: text.into(),
            background: [0, 0, 0, 255],
            foreground: [255, 255, 255, 255],
            outline: None,
            confidence: 1.0,
            font_size: 16,
            weight: FontWeight::Regular,
            italic: false,
            align: TextAlign::Left,
            writing: WritingMode::Horizontal,
        }
    }

    pub fn with_colors(mut self, background: [u8; 4], foreground: [u8; 4]) -> Self {
        self.background = background;
        self.foreground = foreground;
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_font(mut self, size: u32, weight: FontWeight, italic: bool) -> Self {
        self.font_size = size.max(1);
        self.weight = weight;
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

    pub fn with_outline(mut self, outline: [u8; 4]) -> Self {
        self.outline = Some(outline);
        self
    }

    /// Erasure and plate fills fall back when the reading was poor.
    fn needs_plate(&self, style: OverlayStyle) -> bool {
        style == OverlayStyle::Plate || self.confidence < 0.6
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayLayout {
    pub style: OverlayStyle,
    pub blocks: Vec<OverlayBlock>,
    /// 0..=1, applied to everything the overlay draws.
    pub opacity: f32,
}

impl OverlayLayout {
    pub fn new(style: OverlayStyle) -> Self {
        Self {
            style,
            blocks: Vec::new(),
            opacity: 1.0,
        }
    }

    pub fn with_blocks(mut self, blocks: Vec<OverlayBlock>) -> Self {
        self.blocks = blocks;
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }
}

/// Composes overlay frames and tracks which pixels changed since the last one.
///
/// Presenting is charged per pixel, so the compositor reports damage rather than letting the
/// surface upload a full frame every time one tooltip moves. The source frame is what seamless
/// erasure reconstructs from; plate and subtitle styles read their fills from it too, so the
/// overlay always agrees with the pixels underneath.
pub struct Compositor {
    previous: Option<Vec<Rect>>,
    renderer: Renderer,
}

impl std::fmt::Debug for Compositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compositor")
            .field("has_history", &self.previous.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Composition {
    pub frame: Frame,
    /// Regions that must be presented; empty when nothing moved.
    pub damage: Vec<Rect>,
}

/// The transparent value the overlay writes wherever it draws nothing, letting the host application
/// show through untouched.
pub const CLEAR: [u8; 4] = [0, 0, 0, 0];

impl Compositor {
    pub fn new() -> Self {
        Self {
            previous: None,
            renderer: Renderer::new().expect("the bundled fonts ship with the binary"),
        }
    }

    /// Drops the damage history, forcing the next composition to present in full. Used after the
    /// surface is recreated, when its contents are undefined.
    pub fn invalidate(&mut self) {
        self.previous = None;
    }

    /// The regions the previous composition painted.
    ///
    /// The pipeline uses them to decide whether a changed frame can skip re-composing: when no
    /// changed region touches what the overlay already drew, the overlay content is unchanged
    /// and re-presenting it would only risk flicker.
    pub fn last_painted(&self) -> &[Rect] {
        match &self.previous {
            Some(rects) => rects,
            None => &[],
        }
    }

    /// Composes the overlay over `source`. The returned frame is the same size as the source and
    /// fully transparent wherever nothing was drawn.
    pub fn compose(&mut self, source: &Frame, layout: &OverlayLayout) -> Composition {
        let width = source.width();
        let height = source.height();
        let bounds = source.bounds();
        let mut frame = Frame::filled(width, height, CLEAR).expect("overlay dimensions match the source");

        let painted = match layout.style {
            OverlayStyle::Subtitles => self.paint_subtitles(source, &mut frame, layout, bounds),
            _ => self.paint_in_place(source, &mut frame, layout, bounds),
        };

        let damage = match self.previous.take() {
            Some(previous) => coalesce(previous.into_iter().chain(painted.iter().copied()).collect()),
            None => vec![bounds],
        };
        self.previous = Some(painted);

        Composition { frame, damage }
    }

    fn paint_in_place(&mut self, source: &Frame, frame: &mut Frame, layout: &OverlayLayout, bounds: Rect) -> Vec<Rect> {
        let mut painted = Vec::new();
        for block in &layout.blocks {
            if block.text.is_empty() {
                continue;
            }
            let Some(rect) = block.rect.clamp_to(&bounds) else {
                continue;
            };

            if block.needs_plate(layout.style) {
                let alpha = apply_opacity(block.background[3], layout.opacity);
                let fill = [block.background[0], block.background[1], block.background[2], alpha];
                frame.fill_rect(rect, fill);
            } else {
                // Seamless: reconstruct the surface the original text sat on, then write it
                // opaque so the layered window covers the source glyphs completely.
                erase(source, frame, rect);
                if layout.opacity < 1.0 {
                    dim_rect(frame, rect, layout.opacity);
                }
            }

            let origin = (rect.x, rect.y);
            let spec = TextSpec::new(
                block.text.clone(),
                rect.width,
                rect.height.max(block.font_size),
                block.font_size,
            )
            .with_weight(block.weight)
            .with_italic(block.italic)
            .with_align(block.align)
            .with_writing(block.writing);

            if let Ok(ready) = self.renderer.fit(&spec) {
                let ink = self
                    .renderer
                    .draw(frame, &ready, origin, block.foreground, block.outline, layout.opacity);
                let covered = rect.union(&ink.clamp_to(&bounds).unwrap_or(rect));
                painted.push(covered);
            } else {
                painted.push(rect);
            }
        }
        coalesce(painted)
    }

    fn paint_subtitles(
        &mut self,
        source: &Frame,
        frame: &mut Frame,
        layout: &OverlayLayout,
        bounds: Rect,
    ) -> Vec<Rect> {
        let texts: Vec<&OverlayBlock> = layout.blocks.iter().filter(|block| !block.text.is_empty()).collect();
        if texts.is_empty() {
            return Vec::new();
        }
        let line_height = (bounds.height / 18).max(20);
        let band_height = (line_height * texts.len() as u32 + line_height / 2).min(bounds.height);
        let band = Rect::new(
            bounds.x,
            bounds.bottom() - band_height as i32 - (bounds.height / 12) as i32,
            bounds.width,
            band_height,
        );
        let Some(band) = band.clamp_to(&bounds) else {
            return Vec::new();
        };

        // The band covers whatever was under it; reconstruct from the source so a gradient
        // behind the subtitles does not turn into a flat bar at the seam.
        erase(source, frame, band);
        let base = layout.blocks[0].background;
        let overlay_alpha = apply_opacity(220, layout.opacity);
        // Blend the requested plate colour over the reconstruction rather than replacing it.
        for y in band.y as u32..band.bottom() as u32 {
            for x in band.x as u32..band.right() as u32 {
                let under = frame.pixel(x, y);
                let over = [base[0], base[1], base[2], overlay_alpha];
                frame.set_pixel(x, y, blend_straight(under, over));
            }
        }

        let mut y = band.y + (line_height / 4) as i32;
        for block in texts {
            let row = Rect::new(band.x + 8, y, band.width.saturating_sub(16), line_height);
            let spec = TextSpec::new(
                block.text.clone(),
                row.width,
                row.height,
                block.font_size.min(line_height.saturating_sub(2).max(8)),
            )
            .with_weight(block.weight)
            .with_italic(block.italic)
            .with_align(TextAlign::Center)
            .with_writing(block.writing);
            if let Ok(ready) = self.renderer.fit(&spec) {
                self.renderer.draw(
                    frame,
                    &ready,
                    (row.x, row.y),
                    block.foreground,
                    block.outline,
                    layout.opacity,
                );
            }
            y += line_height as i32;
            if y >= band.bottom() {
                break;
            }
        }
        vec![band]
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_opacity(alpha: u8, opacity: f32) -> u8 {
    (alpha as f32 * opacity.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8
}

fn dim_rect(frame: &mut Frame, rect: Rect, opacity: f32) {
    for y in rect.y as u32..rect.bottom() as u32 {
        for x in rect.x as u32..rect.right() as u32 {
            let pixel = frame.pixel(x, y);
            let mut dimmed = pixel;
            dimmed[3] = apply_opacity(pixel[3], opacity);
            frame.set_pixel(x, y, dimmed);
        }
    }
}

fn blend_straight(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
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
        out[channel] = ((sc * sa + dc * da * (1.0 - sa)) / out_a).round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_render::FontWeight;

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

    fn source(width: u32, height: u32) -> Frame {
        Frame::filled(width, height, [30, 30, 30, 255]).expect("dimensions")
    }

    fn layout(style: OverlayStyle) -> OverlayLayout {
        OverlayLayout::new(style).with_blocks(vec![
            OverlayBlock::new(Rect::new(20, 20, 120, 24), "Продолжить игру").with_font(14, FontWeight::Regular, false),
            OverlayBlock::new(Rect::new(20, 60, 90, 24), "Настройки"),
        ])
    }

    #[test]
    fn untouched_pixels_stay_fully_transparent() {
        install_annotations();
        let mut compositor = Compositor::new();
        let composition = compositor.compose(&source(320, 240), &layout(OverlayStyle::Seamless));
        assert_eq!(composition.frame.pixel(300, 220), CLEAR);
    }

    #[test]
    fn the_first_composition_damages_the_whole_surface() {
        install_annotations();
        let mut compositor = Compositor::new();
        let composition = compositor.compose(&source(320, 240), &layout(OverlayStyle::Seamless));
        assert_eq!(composition.damage, vec![Rect::new(0, 0, 320, 240)]);
    }

    #[test]
    fn a_repeated_layout_damages_only_the_painted_regions() {
        install_annotations();
        let mut compositor = Compositor::new();
        let frame = source(320, 240);
        let layout = layout(OverlayStyle::Seamless);
        compositor.compose(&frame, &layout);
        let composition = compositor.compose(&frame, &layout);
        assert!(!composition.damage.is_empty());
        assert!(composition
            .damage
            .iter()
            .all(|rect| rect.width <= 320 && rect.height < 240));
    }

    #[test]
    fn damage_covers_both_the_old_and_the_new_position() {
        install_annotations();
        let mut compositor = Compositor::new();
        let frame = source(320, 240);
        compositor.compose(
            &frame,
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(10, 10, 40, 20), "A")]),
        );
        let composition = compositor.compose(
            &frame,
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(200, 180, 40, 20), "A")]),
        );
        assert!(composition.damage.iter().any(|rect| rect.contains(15, 15)));
        assert!(composition.damage.iter().any(|rect| rect.contains(205, 185)));
    }

    #[test]
    fn invalidating_forces_a_full_present() {
        install_annotations();
        let mut compositor = Compositor::new();
        let frame = source(320, 240);
        let layout = layout(OverlayStyle::Seamless);
        compositor.compose(&frame, &layout);
        compositor.invalidate();
        let composition = compositor.compose(&frame, &layout);
        assert_eq!(composition.damage, vec![Rect::new(0, 0, 320, 240)]);
    }

    #[test]
    fn empty_text_is_not_painted() {
        install_annotations();
        let mut compositor = Compositor::new();
        let composition = compositor.compose(
            &source(320, 240),
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(10, 10, 40, 20), "")]),
        );
        assert_eq!(composition.frame.pixel(20, 20), CLEAR);
    }

    #[test]
    fn low_confidence_blocks_are_plated_at_full_alpha() {
        install_annotations();
        let mut compositor = Compositor::new();
        let frame = source(320, 240);
        let seamless = compositor.compose(
            &frame,
            &OverlayLayout::new(OverlayStyle::Seamless).with_blocks(vec![OverlayBlock::new(
                Rect::new(10, 10, 40, 20),
                "A",
            )
            .with_confidence(0.2)]),
        );
        assert_eq!(seamless.frame.pixel(20, 20)[3], 255);
    }

    #[test]
    fn opacity_scales_what_is_drawn() {
        install_annotations();
        let mut compositor = Compositor::new();
        let frame = source(320, 240);
        let composition = compositor.compose(&frame, &layout(OverlayStyle::Plate).with_opacity(0.5));
        // Plate alpha 255 * 0.5 ≈ 128, modulated by whatever the source shows through.
        let alpha = u32::from(composition.frame.pixel(30, 30)[3]);
        assert!((90..=140).contains(&alpha), "alpha was {alpha}");
    }

    #[test]
    fn blocks_outside_the_surface_are_skipped() {
        install_annotations();
        let mut compositor = Compositor::new();
        let composition = compositor.compose(
            &source(100, 100),
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(400, 400, 40, 20), "A")]),
        );
        assert_eq!(composition.damage, vec![Rect::new(0, 0, 100, 100)]);
        assert!(composition.frame.as_bytes().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn subtitles_draw_one_band_near_the_bottom() {
        install_annotations();
        let mut compositor = Compositor::new();
        let frame = source(640, 480);
        let composition = compositor.compose(&frame, &layout(OverlayStyle::Subtitles));
        let band = *composition.damage.first().unwrap();
        assert_eq!(composition.damage.len(), 1);
        assert_eq!(band, Rect::new(0, 0, 640, 480));
        let second = compositor.compose(&frame, &layout(OverlayStyle::Subtitles));
        let band = *second.damage.first().unwrap();
        assert_eq!(band.width, 640);
        assert!(band.y > 300);
        assert!(band.bottom() <= 480);
    }

    #[test]
    fn a_layout_round_trips_through_json() {
        install_annotations();
        let layout = layout(OverlayStyle::Plate).with_opacity(0.8);
        let text = serde_json::to_string(&layout).unwrap();
        assert_eq!(serde_json::from_str::<OverlayLayout>(&text).unwrap(), layout);
    }

    #[test]
    fn seamless_erasure_covers_the_source_box_with_reconstruction() {
        install_annotations();
        let mut compositor = Compositor::new();
        // A gradient source: flat fills would be obvious.
        let width = 120;
        let height = 40;
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _y in 0..height {
            for x in 0..width {
                let shade = (x * 2) as u8;
                pixels.extend_from_slice(&[shade, shade, shade, 255]);
            }
        }
        let frame = Frame::packed(width, height, pixels).expect("dimensions");
        let composition = compositor.compose(
            &frame,
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(20, 10, 60, 20), "Hello")]),
        );
        // Inside the erased box the overlay is opaque (it has to cover the original glyphs).
        let centre = composition.frame.pixel(50, 20);
        assert_eq!(centre[3], 255, "erasure must cover the source");
    }

    #[test]
    fn a_composition_is_deterministic() {
        install_annotations();
        let compose = || {
            let mut compositor = Compositor::new();
            let frame = source(200, 80);
            compositor
                .compose(&frame, &layout(OverlayStyle::Seamless))
                .frame
                .as_bytes()
                .to_vec()
        };
        assert_eq!(compose(), compose());
    }

    #[test]
    fn last_painted_reports_the_previous_composition() {
        install_annotations();
        let mut compositor = Compositor::new();
        let frame = source(64, 64);
        assert!(compositor.last_painted().is_empty());

        let layout = OverlayLayout::new(OverlayStyle::Plate)
            .with_blocks(vec![OverlayBlock::new(Rect::new(8, 8, 32, 16), "text")]);
        compositor.compose(&frame, &layout);
        assert_eq!(compositor.last_painted(), &[Rect::new(8, 8, 32, 16)]);
    }

    #[test]
    fn an_empty_layout_after_a_drawn_one_damages_the_drawn_region() {
        install_annotations();
        // The clear path: an empty overlay over what was drawn must damage exactly what has to
        // be erased, so fail-open can hand it to the surface.
        let mut compositor = Compositor::new();
        let frame = source(64, 64);
        let drawn = OverlayLayout::new(OverlayStyle::Plate)
            .with_blocks(vec![OverlayBlock::new(Rect::new(8, 8, 32, 16), "text")]);
        compositor.compose(&frame, &drawn);

        let cleared = compositor.compose(&frame, &OverlayLayout::new(OverlayStyle::Plate));
        assert_eq!(cleared.damage, vec![Rect::new(8, 8, 32, 16)]);
        assert_eq!(cleared.frame.pixel(10, 10), CLEAR);
    }

    #[test]
    fn blocks_with_no_text_and_blocks_outside_the_frame_are_not_drawn() {
        install_annotations();
        let mut compositor = Compositor::new();
        let frame = source(64, 64);
        let layout = OverlayLayout::new(OverlayStyle::Seamless).with_blocks(vec![
            OverlayBlock::new(Rect::new(8, 8, 32, 16), ""),
            OverlayBlock::new(Rect::new(-40, -40, 16, 16), "away"),
        ]);

        let composition = compositor.compose(&frame, &layout);
        assert!(composition.frame.as_bytes().iter().all(|byte| *byte == 0), "nothing was drawn");
        assert!(compositor.last_painted().is_empty());
    }
}
