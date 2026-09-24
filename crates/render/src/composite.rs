//! Full compositor: erase original text → fit translation → draw translated text.
//!
//! This is the complete pipeline for pixel-perfect overlay rendering:
//! 1. **Sample** — measure the original text colour and background from source pixels
//! 2. **Erase** — inpaint the original text region (reconstruct background)
//! 3. **Fit** — find the optimal font size for the translated text
//! 4. **Draw** — render the translated text in the same position, colour, and style
//!
//! The result is a seamless overlay where the translation looks like it was
//! always there — same position, same size, same colour, same weight.

use lumen_core::{Frame, Rect};

use crate::fit::{TextSpec};
use crate::inpaint::inpaint;
use crate::text::{FontWeight, Renderer, TextAlign};
use crate::writing::WritingMode;

/// Configuration for the compositor.
#[derive(Debug, Clone)]
pub struct CompositeConfig {
    /// Whether to erase the original text before drawing.
    pub erase_original: bool,
    /// Padding around the text region for inpainting (in pixels).
    pub inpaint_padding: u32,
    /// Whether to auto-detect text colour from source pixels.
    pub auto_colour: bool,
    /// Minimum contrast ratio between text and background (WCAG AA = 4.5).
    pub min_contrast: f32,
}

impl Default for CompositeConfig {
    fn default() -> Self {
        Self {
            erase_original: true,
            inpaint_padding: 4,
            auto_colour: true,
            min_contrast: 3.0,
        }
    }
}

/// One block to composite onto the frame.
#[derive(Debug, Clone)]
pub struct CompositeBlock {
    /// Screen rectangle of the original text (pixel-perfect bounds from OCR).
    pub rect: Rect,
    /// The translated text to draw.
    pub text: String,
    /// Preferred font size (measured from original text height).
    pub font_size: u32,
    /// Text colour (BGRA). If [0,0,0,0], auto-detected from source.
    pub colour: [u8; 4],
    /// Font weight.
    pub weight: FontWeight,
    /// Text alignment within the bubble.
    pub align: TextAlign,
}

/// Result of compositing one block.
#[derive(Debug, Clone)]
pub struct CompositeResult {
    /// The rectangle that was actually drawn.
    pub drawn_rect: Rect,
    /// Font size that was used.
    pub font_size: f32,
    /// Whether the text overflowed the box.
    pub overflow: bool,
    /// Detected text colour.
    pub colour: [u8; 4],
}

/// Composites all blocks onto the frame: sample → erase → fit → draw.
///
/// This is the main entry point for rendering translated overlays.
/// It modifies the frame in place, erasing original text and drawing translations
/// in the exact same position and style.
pub fn composite(
    frame: &mut Frame,
    blocks: &[CompositeBlock],
    renderer: &mut Renderer,
    config: &CompositeConfig,
) -> Vec<CompositeResult> {
    let mut results = Vec::with_capacity(blocks.len());

    for block in blocks {
        let result = composite_one(frame, block, renderer, config);
        results.push(result);
    }

    results
}

/// Composites one block onto the frame.
fn composite_one(
    frame: &mut Frame,
    block: &CompositeBlock,
    renderer: &mut Renderer,
    config: &CompositeConfig,
) -> CompositeResult {
    // Step 1: Sample the original text colour from source pixels
    let colour = if config.auto_colour && block.colour == [0, 0, 0, 0] {
        sample_text_colour(frame, block.rect)
    } else {
        block.colour
    };

    // Step 2: Erase the original text via inpainting
    if config.erase_original {
        let padded_rect = Rect::new(
            block.rect.x.saturating_sub(config.inpaint_padding as i32),
            block.rect.y.saturating_sub(config.inpaint_padding as i32),
            block.rect.width + config.inpaint_padding * 2,
            block.rect.height + config.inpaint_padding * 2,
        );
        let inpainted = inpaint(frame, padded_rect);
        write_pixels(frame, padded_rect, &inpainted);
    }

    // Step 3: Fit the translated text into the original box
    let spec = TextSpec::new(
        &block.text,
        block.rect.width,
        block.rect.height,
        block.font_size,
    )
    .with_weight(block.weight)
    .with_align(block.align)
    .with_writing(WritingMode::Horizontal);

    let ready = match renderer.fit(&spec) {
        Ok(ready) => ready,
        Err(_) => {
            // If fitting fails entirely, draw at preferred size anyway
            let fallback_spec = TextSpec::new(
                &block.text,
                block.rect.width,
                block.rect.height,
                block.font_size.max(8),
            )
            .with_weight(block.weight)
            .with_align(block.align);
            match renderer.fit(&fallback_spec) {
                Ok(r) => r,
                Err(_) => {
                    return CompositeResult {
                        drawn_rect: block.rect,
                        font_size: block.font_size as f32,
                        overflow: true,
                        colour,
                    };
                }
            }
        }
    };

    // Step 4: Draw the translated text at the EXACT same position
    let origin = (block.rect.x, block.rect.y);
    let drawn_rect = renderer.draw(
        frame,
        &ready,
        origin,
        colour,
        None, // No outline for clean comic text
        1.0,  // Full opacity
    );

    CompositeResult {
        drawn_rect,
        font_size: ready.fitted.size,
        overflow: ready.fitted.at_floor,
        colour,
    }
}

/// Samples the dominant text colour from a region by finding the darkest/lightest
/// pixels that contrast with the background.
fn sample_text_colour(frame: &Frame, rect: Rect) -> [u8; 4] {
    let bounds = frame.bounds();
    let clamped = match rect.clamp_to(&bounds) {
        Some(r) => r,
        None => return [0, 0, 0, 255],
    };

    // Collect all pixel luminances
    let mut pixels: Vec<(u32, u32, u8)> = Vec::new();
    for y in 0..clamped.height {
        for x in 0..clamped.width {
            let px = (clamped.x + x as i32) as u32;
            let py = (clamped.y + y as i32) as u32;
            let bgra = frame.pixel(px, py);
            let luminance = (bgra[2] as u32 * 299 + bgra[1] as u32 * 587 + bgra[0] as u32 * 114) / 1000;
            pixels.push((px, py, luminance as u8));
        }
    }

    if pixels.is_empty() {
        return [0, 0, 0, 255];
    }

    // Estimate background as the most common luminance (modal)
    let mut luminance_histogram = [0u32; 256];
    for &(_, _, lum) in &pixels {
        luminance_histogram[lum as usize] += 1;
    }
    let bg_luminance = luminance_histogram
        .iter()
        .enumerate()
        .max_by_key(|(_, &count)| count)
        .map(|(lum, _)| lum as u8)
        .unwrap_or(200);

    // Text is the pixels that contrast most with the background
    let is_dark_bg = bg_luminance < 128;
    let text_pixels: Vec<(u32, u32)> = pixels
        .iter()
        .filter(|&&(_, _, lum)| {
            if is_dark_bg {
                lum > bg_luminance + 40 // Light text on dark bg
            } else {
                lum < bg_luminance.saturating_sub(40) // Dark text on light bg
            }
        })
        .map(|&(px, py, _)| (px, py))
        .collect();

    if text_pixels.is_empty() {
        // Fallback: use black text on light bg, white text on dark bg
        return if is_dark_bg {
            [255, 255, 255, 255]
        } else {
            [0, 0, 0, 255]
        };
    }

    // Average the text pixel colours
    let mut b_sum = 0u64;
    let mut g_sum = 0u64;
    let mut r_sum = 0u64;
    let count = text_pixels.len() as u64;

    for &(px, py) in &text_pixels {
        let bgra = frame.pixel(px, py);
        b_sum += bgra[0] as u64;
        g_sum += bgra[1] as u64;
        r_sum += bgra[2] as u64;
    }

    [
        (b_sum / count) as u8,
        (g_sum / count) as u8,
        (r_sum / count) as u8,
        255,
    ]
}

/// Writes pixels back to the frame.
fn write_pixels(frame: &mut Frame, rect: Rect, pixels: &[[u8; 4]]) {
    let bounds = frame.bounds();
    let clamped = match rect.clamp_to(&bounds) {
        Some(r) => r,
        None => return,
    };

    for y in 0..clamped.height {
        for x in 0..clamped.width {
            let src_x = (clamped.x - rect.x) as u32 + x;
            let src_y = (clamped.y - rect.y) as u32 + y;
            let src_idx = (src_y * rect.width + src_x) as usize;
            if src_idx < pixels.len() {
                let px = (clamped.x + x as i32) as u32;
                let py = (clamped.y + y as i32) as u32;
                frame.set_pixel(px, py, pixels[src_idx]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_config_defaults() {
        let config = CompositeConfig::default();
        assert!(config.erase_original);
        assert_eq!(config.inpaint_padding, 4);
        assert!(config.auto_colour);
    }

    #[test]
    fn sample_text_colour_fallback() {
        let frame = Frame::filled(100, 100, [255, 255, 255, 255]).unwrap();
        let colour = sample_text_colour(&frame, Rect::new(0, 0, 50, 50));
        // All white pixels — should fall back to black text
        assert_eq!(colour, [0, 0, 0, 255]);
    }

    #[test]
    fn sample_text_colour_dark_text_on_light_bg() {
        let mut frame = Frame::filled(100, 100, [255, 255, 255, 255]).unwrap();
        // Draw some dark pixels (simulating text)
        for x in 10..30 {
            for y in 10..20 {
                frame.set_pixel(x, y, [0, 0, 0, 255]);
            }
        }
        let colour = sample_text_colour(&frame, Rect::new(0, 0, 50, 50));
        // Should detect dark text
        assert!(colour[2] < 100, "expected dark text colour, got {:?}", colour);
    }
}
