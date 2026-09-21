use lumen_core::{coalesce, Frame, Rect};
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
///
/// Typesetting arrives in M4; at this stage a block carries the geometry and the sampled colours,
/// which is what the erasure and damage-tracking logic operates on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayBlock {
    pub rect: Rect,
    pub text: String,
    pub background: [u8; 4],
    pub foreground: [u8; 4],
    /// Recognition confidence in 0..=1. Low-confidence blocks fall back to a plate because erasing
    /// text that was read badly is worse than covering it.
    pub confidence: f32,
}

impl OverlayBlock {
    pub fn new(rect: Rect, text: impl Into<String>) -> Self {
        Self {
            rect,
            text: text.into(),
            background: [0, 0, 0, 255],
            foreground: [255, 255, 255, 255],
            confidence: 1.0,
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
/// surface upload a full frame every time one tooltip moves.
#[derive(Debug, Default)]
pub struct Compositor {
    previous: Option<Vec<Rect>>,
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
        Self::default()
    }

    /// Drops the damage history, forcing the next composition to present in full. Used after the
    /// surface is recreated, when its contents are undefined.
    pub fn invalidate(&mut self) {
        self.previous = None;
    }

    pub fn compose(&mut self, width: u32, height: u32, layout: &OverlayLayout) -> Composition {
        let bounds = Rect::new(0, 0, width, height);
        let mut frame = Frame::filled(width, height, CLEAR).expect("overlay dimensions are validated by the surface");

        let painted = match layout.style {
            OverlayStyle::Subtitles => self.paint_subtitles(&mut frame, layout, bounds),
            _ => self.paint_in_place(&mut frame, layout, bounds),
        };

        let damage = match self.previous.take() {
            Some(previous) => coalesce(previous.into_iter().chain(painted.iter().copied()).collect()),
            None => vec![bounds],
        };
        self.previous = Some(painted);

        Composition { frame, damage }
    }

    fn paint_in_place(&self, frame: &mut Frame, layout: &OverlayLayout, bounds: Rect) -> Vec<Rect> {
        let mut painted = Vec::new();
        for block in &layout.blocks {
            if block.text.is_empty() {
                continue;
            }
            let Some(rect) = block.rect.clamp_to(&bounds) else {
                continue;
            };
            // A plate is requested explicitly, and is also the automatic fallback when the source
            // pixels were read too poorly to erase them convincingly.
            let plate = layout.style == OverlayStyle::Plate || block.confidence < 0.6;
            let alpha = apply_opacity(block.background[3], layout.opacity);
            let fill = if plate {
                [block.background[0], block.background[1], block.background[2], alpha]
            } else {
                [
                    block.background[0],
                    block.background[1],
                    block.background[2],
                    apply_opacity(block.background[3], layout.opacity * 0.92),
                ]
            };
            frame.fill_rect(rect, fill);
            painted.push(rect);
        }
        coalesce(painted)
    }

    fn paint_subtitles(&self, frame: &mut Frame, layout: &OverlayLayout, bounds: Rect) -> Vec<Rect> {
        let lines = layout.blocks.iter().filter(|block| !block.text.is_empty()).count() as u32;
        if lines == 0 {
            return Vec::new();
        }
        let line_height = (bounds.height / 18).max(20);
        let band_height = (line_height * lines + line_height / 2).min(bounds.height);
        let band = Rect::new(
            bounds.x,
            bounds.bottom() - band_height as i32 - (bounds.height / 12) as i32,
            bounds.width,
            band_height,
        );
        let Some(band) = band.clamp_to(&bounds) else {
            return Vec::new();
        };
        let background = layout.blocks[0].background;
        frame.fill_rect(
            band,
            [
                background[0],
                background[1],
                background[2],
                apply_opacity(220, layout.opacity),
            ],
        );
        vec![band]
    }
}

fn apply_opacity(alpha: u8, opacity: f32) -> u8 {
    (alpha as f32 * opacity.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(style: OverlayStyle) -> OverlayLayout {
        OverlayLayout::new(style).with_blocks(vec![
            OverlayBlock::new(Rect::new(20, 20, 120, 24), "Продолжить игру"),
            OverlayBlock::new(Rect::new(20, 60, 90, 24), "Настройки"),
        ])
    }

    #[test]
    fn untouched_pixels_stay_fully_transparent() {
        let mut compositor = Compositor::new();
        let composition = compositor.compose(320, 240, &layout(OverlayStyle::Seamless));
        assert_eq!(composition.frame.pixel(300, 220), CLEAR);
    }

    #[test]
    fn the_first_composition_damages_the_whole_surface() {
        let mut compositor = Compositor::new();
        let composition = compositor.compose(320, 240, &layout(OverlayStyle::Seamless));
        assert_eq!(composition.damage, vec![Rect::new(0, 0, 320, 240)]);
    }

    #[test]
    fn a_repeated_layout_damages_only_the_painted_regions() {
        let mut compositor = Compositor::new();
        let layout = layout(OverlayStyle::Seamless);
        compositor.compose(320, 240, &layout);
        let composition = compositor.compose(320, 240, &layout);
        assert!(!composition.damage.is_empty());
        assert!(composition.damage.iter().all(|rect| rect.width <= 320 && rect.height < 240));
    }

    #[test]
    fn damage_covers_both_the_old_and_the_new_position() {
        let mut compositor = Compositor::new();
        compositor.compose(
            320,
            240,
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(10, 10, 40, 20), "A")]),
        );
        let composition = compositor.compose(
            320,
            240,
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(200, 180, 40, 20), "A")]),
        );
        assert!(composition.damage.iter().any(|rect| rect.contains(15, 15)));
        assert!(composition.damage.iter().any(|rect| rect.contains(205, 185)));
    }

    #[test]
    fn invalidating_forces_a_full_present() {
        let mut compositor = Compositor::new();
        let layout = layout(OverlayStyle::Seamless);
        compositor.compose(320, 240, &layout);
        compositor.invalidate();
        let composition = compositor.compose(320, 240, &layout);
        assert_eq!(composition.damage, vec![Rect::new(0, 0, 320, 240)]);
    }

    #[test]
    fn empty_text_is_not_painted() {
        let mut compositor = Compositor::new();
        let composition = compositor.compose(
            320,
            240,
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(10, 10, 40, 20), "")]),
        );
        assert_eq!(composition.frame.pixel(20, 20), CLEAR);
    }

    #[test]
    fn low_confidence_blocks_are_plated_at_full_alpha() {
        let mut compositor = Compositor::new();
        let seamless = compositor.compose(
            320,
            240,
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(10, 10, 40, 20), "A").with_confidence(0.2)]),
        );
        assert_eq!(seamless.frame.pixel(20, 20)[3], 255);
    }

    #[test]
    fn opacity_scales_what_is_drawn() {
        let mut compositor = Compositor::new();
        let composition = compositor.compose(320, 240, &layout(OverlayStyle::Plate).with_opacity(0.5));
        assert_eq!(composition.frame.pixel(30, 30)[3], 128);
    }

    #[test]
    fn blocks_outside_the_surface_are_skipped() {
        let mut compositor = Compositor::new();
        let composition = compositor.compose(
            100,
            100,
            &OverlayLayout::new(OverlayStyle::Seamless)
                .with_blocks(vec![OverlayBlock::new(Rect::new(400, 400, 40, 20), "A")]),
        );
        assert_eq!(composition.damage, vec![Rect::new(0, 0, 100, 100)]);
        assert!(composition.frame.as_bytes().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn subtitles_draw_one_band_near_the_bottom() {
        let mut compositor = Compositor::new();
        let composition = compositor.compose(640, 480, &layout(OverlayStyle::Subtitles));
        let band = *composition.damage.first().unwrap();
        assert_eq!(composition.damage.len(), 1);
        assert_eq!(band, Rect::new(0, 0, 640, 480));
        let second = compositor.compose(640, 480, &layout(OverlayStyle::Subtitles));
        let band = *second.damage.first().unwrap();
        assert_eq!(band.width, 640);
        assert!(band.y > 300);
        assert!(band.bottom() <= 480);
    }

    #[test]
    fn a_layout_round_trips_through_json() {
        let layout = layout(OverlayStyle::Plate).with_opacity(0.8);
        let text = serde_json::to_string(&layout).unwrap();
        assert_eq!(serde_json::from_str::<OverlayLayout>(&text).unwrap(), layout);
    }
}
