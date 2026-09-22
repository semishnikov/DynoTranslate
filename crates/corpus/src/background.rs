//! What the corpus text stands on.
//!
//! Recognition is measured over the range of backgrounds a real screen shows: a flat panel, a
//! gradient wash, and texture. Every one of them is generated from data in the spec, so a scene
//! with the same seed has pixel-identical background anywhere it is rendered.

use lumen_core::Frame;
use serde::{Deserialize, Serialize};

use crate::rng::Rng;

/// Which family of background a scene uses, the axis the corpus reports recognition over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundKind {
    Solid,
    Gradient,
    Noise,
}

impl BackgroundKind {
    pub fn all() -> &'static [BackgroundKind] {
        &[BackgroundKind::Solid, BackgroundKind::Gradient, BackgroundKind::Noise]
    }

    pub fn name(self) -> &'static str {
        match self {
            BackgroundKind::Solid => "solid",
            BackgroundKind::Gradient => "gradient",
            BackgroundKind::Noise => "noise",
        }
    }
}

/// A concrete background, colours and seed included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Background {
    Solid { color: [u8; 4] },
    Gradient { top: [u8; 4], bottom: [u8; 4] },
    /// `base` with per-pixel grain of up to `amplitude` per channel, from `seed`.
    Noise { base: [u8; 4], amplitude: u8, seed: u64 },
}

impl Background {
    pub fn solid(color: [u8; 4]) -> Self {
        Background::Solid { color }
    }

    pub fn kind(&self) -> BackgroundKind {
        match self {
            Background::Solid { .. } => BackgroundKind::Solid,
            Background::Gradient { .. } => BackgroundKind::Gradient,
            Background::Noise { .. } => BackgroundKind::Noise,
        }
    }

    /// Overwrites the whole frame.
    pub fn paint(&self, frame: &mut Frame) {
        match self {
            Background::Solid { color } => frame.fill_rect(frame.bounds(), *color),
            Background::Gradient { top, bottom } => paint_gradient(frame, *top, *bottom),
            Background::Noise { base, amplitude, seed } => paint_noise(frame, *base, *amplitude, *seed),
        }
    }

    /// The colour the background settles around, which is what an inpainting pass would want.
    pub fn base_color(&self) -> [u8; 4] {
        match self {
            Background::Solid { color } => *color,
            Background::Gradient { top, bottom } => mix(*top, *bottom, 0.5),
            Background::Noise { base, .. } => *base,
        }
    }
}

fn paint_gradient(frame: &mut Frame, top: [u8; 4], bottom: [u8; 4]) {
    let height = frame.height();
    for y in 0..height {
        // A one-row frame divides by max(1) and its only y is 0, so t is the zero it was.
        let t = y as f32 / (height - 1).max(1) as f32;
        let color = mix(top, bottom, t);
        for x in 0..frame.width() {
            frame.set_pixel(x, y, color);
        }
    }
}

fn paint_noise(frame: &mut Frame, base: [u8; 4], amplitude: u8, seed: u64) {
    let mut rng = Rng::new(seed);
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let grain = rng.next_u64();
            let spread = u64::from(amplitude) * 2 + 1;
            let color = std::array::from_fn(|channel| {
                if channel == 3 {
                    return base[3];
                }
                let offset = ((grain >> (channel * 16)) % spread) as i32 - i32::from(amplitude);
                (i32::from(base[channel]) + offset).clamp(0, 255) as u8
            });
            frame.set_pixel(x, y, color);
        }
    }
}

/// Per-channel linear interpolation, rounded, with `t` clamped to `0.0..=1.0`.
fn mix(left: [u8; 4], right: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|channel| {
        let from = f32::from(left[channel]);
        let to = f32::from(right[channel]);
        (from + (to - from) * t).round() as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame::filled(32, 16, [0, 0, 0, 0]).unwrap()
    }

    #[test]
    fn solid_paints_every_pixel() {
        let mut target = frame();
        Background::solid([10, 20, 30, 255]).paint(&mut target);
        for y in 0..target.height() {
            for x in 0..target.width() {
                assert_eq!(target.pixel(x, y), [10, 20, 30, 255]);
            }
        }
        assert_eq!(Background::solid([1, 2, 3, 4]).kind(), BackgroundKind::Solid);
    }

    #[test]
    fn a_gradient_hits_its_endpoints_exactly() {
        let mut target = frame();
        let top = [200, 100, 50, 255];
        let bottom = [20, 40, 60, 255];
        Background::Gradient { top, bottom }.paint(&mut target);
        assert_eq!(target.pixel(0, 0), top);
        assert_eq!(target.pixel(31, 15), bottom);
        // Every row is one colour, and the rows move monotonically from top to bottom.
        let first_channel = |y: u32| target.pixel(0, y)[0];
        let middle = first_channel(8);
        assert!(first_channel(0) >= middle && middle >= first_channel(15));
    }

    #[test]
    fn noise_is_bounded_by_its_amplitude_and_its_seed() {
        let base = [128, 64, 200, 255];
        let mut left = frame();
        let mut right = frame();
        let mut other = frame();
        let noise = Background::Noise {
            base,
            amplitude: 12,
            seed: 7,
        };
        let other_seed = Background::Noise {
            base,
            amplitude: 12,
            seed: 8,
        };
        noise.paint(&mut left);
        noise.paint(&mut right);
        other_seed.paint(&mut other);

        assert_eq!(left, right);
        assert_ne!(left, other);
        for y in 0..left.height() {
            for x in 0..left.width() {
                let pixel = left.pixel(x, y);
                for channel in 0..3 {
                    let distance = i32::from(pixel[channel]) - i32::from(base[channel]);
                    assert!(distance.abs() <= 12, "channel {channel} drifted by {distance}");
                }
                assert_eq!(pixel[3], 255);
            }
        }
    }

    #[test]
    fn noise_actually_varies_pixel_to_pixel() {
        let mut target = frame();
        let grain = Background::Noise {
            base: [100, 100, 100, 255],
            amplitude: 20,
            seed: 3,
        };
        grain.paint(&mut target);
        let first = target.pixel(0, 0);
        let varies = (0..target.width()).any(|x| target.pixel(x, 0) != first);
        assert!(varies);
    }

    #[test]
    fn mix_round_trips_its_endpoints() {
        let left = [0, 10, 20, 255];
        let right = [100, 110, 120, 255];
        assert_eq!(mix(left, right, 0.0), left);
        assert_eq!(mix(left, right, 1.0), right);
        assert_eq!(mix(left, right, 0.5), [50, 60, 70, 255]);
    }

    #[test]
    fn base_color_reports_the_middle_of_every_kind() {
        assert_eq!(Background::solid([1, 2, 3, 4]).base_color(), [1, 2, 3, 4]);
        let gradient = Background::Gradient {
            top: [0, 0, 0, 255],
            bottom: [100, 100, 100, 255],
        };
        assert_eq!(gradient.base_color(), [50, 50, 50, 255]);
        let noise = Background::Noise {
            base: [7, 8, 9, 255],
            amplitude: 5,
            seed: 1,
        };
        assert_eq!(noise.base_color(), [7, 8, 9, 255]);
    }

    #[test]
    fn kinds_are_enumerable_for_the_plan() {
        assert_eq!(BackgroundKind::all().len(), 3);
        for kind in BackgroundKind::all() {
            assert!(!kind.name().is_empty());
        }
    }
}
