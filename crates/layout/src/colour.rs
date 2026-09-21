//! Colour sampling for a block of text.
//!
//! The compositor needs two colours per block: what to erase the original with, and what to draw
//! the translation in. Both are measured from the pixels the application already drew. Guessing is
//! not an option, because a translation sitting on the wrong background is more visible than no
//! translation at all.

use lumen_core::{Frame, Rect};

/// Returned when a region has no pixels to measure.
pub const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];

/// Samples taken along the longest side of a region. Enough to see past anti-aliasing, few enough
/// that measuring a full-screen block costs no more than measuring a tooltip.
const SAMPLES_PER_SIDE: u32 = 64;

/// Squared channel distance below which two samples count as the same colour. Glyph edges produce a
/// gradient between the fill and the surface behind them, and treating every shade as its own
/// colour would make the most common one a shade nobody can read.
const SAME_COLOUR: u32 = 32 * 32 * 3;

/// Width of the ring measured around a text rectangle to find the surface it sits on.
const RING: u32 = 3;

/// The colour most of `region` is painted in, or [`TRANSPARENT`] if the region has no pixels.
pub fn modal_colour(frame: &Frame, region: Rect) -> [u8; 4] {
    tally(frame, region, Rect::new(0, 0, 0, 0))
}

/// The surface a block sits on and the colour its text is drawn in.
///
/// The background comes from a ring just outside the text, which is where the surface shows through
/// untouched. The foreground is the most common colour inside the text that is not that background,
/// which is the glyph fill rather than its anti-aliased edge. When the text is the same colour as
/// its surroundings there is nothing to measure, and the fallback is whichever of black and white
/// the background can carry.
pub fn sample_pair(frame: &Frame, bounds: Rect) -> ([u8; 4], [u8; 4]) {
    let background = tally(frame, bounds.inflate(RING), bounds);
    (background, ink(frame, bounds, background))
}

/// Relative brightness in 0..=255.
pub fn luminance(colour: [u8; 4]) -> u32 {
    let [blue, green, red, _] = colour;
    (299 * u32::from(red) + 587 * u32::from(green) + 114 * u32::from(blue)) / 1000
}

/// Whether two colours are close enough to be treated as one.
pub fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    distance(a, b) <= SAME_COLOUR
}

fn distance(a: [u8; 4], b: [u8; 4]) -> u32 {
    let mut sum = 0;
    for (x, y) in a.iter().zip(b.iter()).take(3) {
        let delta = i32::from(*x) - i32::from(*y);
        sum += (delta * delta) as u32;
    }
    sum
}

/// The most common colour in `region`, ignoring anything inside `excluded`.
fn tally(frame: &Frame, region: Rect, excluded: Rect) -> [u8; 4] {
    let Some(region) = region.clamp_to(&frame.bounds()) else {
        return TRANSPARENT;
    };

    let step = sample_step(region);
    let mut buckets: Vec<([u8; 4], u32)> = Vec::new();
    for y in (region.y..region.bottom()).step_by(step) {
        for x in (region.x..region.right()).step_by(step) {
            if excluded.contains(x, y) {
                continue;
            }
            record(&mut buckets, frame.pixel(x as u32, y as u32));
        }
    }
    winner(&buckets)
}

/// The colour inside `bounds` that stands out most from `background`.
fn ink(frame: &Frame, bounds: Rect, background: [u8; 4]) -> [u8; 4] {
    let Some(bounds) = bounds.clamp_to(&frame.bounds()) else {
        return fallback_ink(background);
    };

    let step = sample_step(bounds);
    let mut buckets: Vec<([u8; 4], u32)> = Vec::new();
    for y in (bounds.y..bounds.bottom()).step_by(step) {
        for x in (bounds.x..bounds.right()).step_by(step) {
            let colour = frame.pixel(x as u32, y as u32);
            if !near(colour, background) {
                record(&mut buckets, colour);
            }
        }
    }

    let strongest = winner(&buckets);
    if strongest == TRANSPARENT {
        fallback_ink(background)
    } else {
        strongest
    }
}

/// Sampling stride that keeps the sample count bounded whatever the region's size.
fn sample_step(region: Rect) -> usize {
    let longest = region.width.max(region.height);
    ((longest / SAMPLES_PER_SIDE) as usize).max(1)
}

/// Adds a sample to the closest bucket it already has, or opens a bucket for it.
fn record(buckets: &mut Vec<([u8; 4], u32)>, colour: [u8; 4]) {
    match buckets.iter_mut().find(|(seen, _)| near(*seen, colour)) {
        Some((_, count)) => *count += 1,
        None => buckets.push((colour, 1)),
    }
}

/// The most sampled colour. Ties go to the darker one, so the result never depends on the order the
/// pixels happened to be visited in.
fn winner(buckets: &[([u8; 4], u32)]) -> [u8; 4] {
    buckets
        .iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map_or(TRANSPARENT, |(colour, _)| *colour)
}

fn fallback_ink(background: [u8; 4]) -> [u8; 4] {
    if luminance(background) > 127 {
        [0, 0, 0, 255]
    } else {
        [255, 255, 255, 255]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACE: [u8; 4] = [12, 12, 12, 255];
    const GLYPH: [u8; 4] = [240, 240, 240, 255];

    fn scene(width: u32, height: u32, painted: Rect, colour: [u8; 4]) -> Frame {
        let mut frame = Frame::filled(width, height, SURFACE).expect("valid dimensions");
        frame.fill_rect(painted, colour);
        frame
    }

    #[test]
    fn the_dominant_colour_of_a_uniform_region_is_that_colour() {
        let frame = scene(64, 64, Rect::new(0, 0, 0, 0), GLYPH);

        assert_eq!(modal_colour(&frame, frame.bounds()), SURFACE);
    }

    #[test]
    fn an_empty_region_has_no_colour_to_report() {
        let frame = scene(64, 64, Rect::new(0, 0, 0, 0), GLYPH);

        assert_eq!(modal_colour(&frame, Rect::new(200, 200, 40, 40)), TRANSPARENT);
    }

    #[test]
    fn a_block_reports_the_surface_around_it_and_the_ink_inside_it() {
        let text = Rect::new(20, 20, 24, 8);
        let frame = scene(64, 64, text, GLYPH);

        assert_eq!(sample_pair(&frame, text), (SURFACE, GLYPH));
    }

    #[test]
    fn text_the_same_colour_as_its_surface_falls_back_to_a_readable_default() {
        let text = Rect::new(8, 8, 16, 8);
        let frame = scene(64, 64, Rect::new(0, 0, 64, 64), [250, 250, 250, 255]);

        let (background, foreground) = sample_pair(&frame, text);

        assert_eq!(background, [250, 250, 250, 255]);
        assert_eq!(foreground, [0, 0, 0, 255]);
    }

    #[test]
    fn a_dark_surface_falls_back_to_light_text_and_a_light_one_to_dark() {
        assert_eq!(fallback_ink(SURFACE), [255, 255, 255, 255]);
        assert_eq!(fallback_ink([250, 250, 250, 255]), [0, 0, 0, 255]);
    }

    #[test]
    fn brightness_orders_black_below_grey_below_white() {
        assert!(luminance([0, 0, 0, 255]) < luminance([128, 128, 128, 255]));
        assert!(luminance([128, 128, 128, 255]) < luminance([255, 255, 255, 255]));
    }

    #[test]
    fn close_shades_are_one_colour_and_distant_ones_are_not() {
        assert!(near([100, 100, 100, 255], [110, 100, 100, 255]));
        assert!(!near([100, 100, 100, 255], [200, 100, 100, 255]));
    }
}
