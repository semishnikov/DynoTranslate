use lumen_core::{Frame, Rect};

#[derive(Debug, thiserror::Error)]
pub enum SurfaceError {
    #[error("the surface was destroyed")]
    Gone,
    #[error("the composited frame is {width}x{height}, the surface is {surface_width}x{surface_height}")]
    SizeMismatch {
        width: u32,
        height: u32,
        surface_width: u32,
        surface_height: u32,
    },
    #[error("{operation} failed: {detail}")]
    Platform { operation: &'static str, detail: String },
}

/// Properties the overlay must hold for the product's safety promises to be true.
///
/// They are represented explicitly so they can be asserted rather than assumed: an overlay that
/// takes focus interrupts a game, and an overlay that its own capture can see feeds its output back
/// into recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceProperties {
    /// Clicks and keystrokes pass through to the application underneath.
    pub click_through: bool,
    /// Never activated, so the foreground application keeps focus.
    pub never_activates: bool,
    /// Absent from Alt-Tab and the taskbar.
    pub hidden_from_switcher: bool,
    /// Excluded from screen capture, including the pipeline's own.
    pub excluded_from_capture: bool,
    pub topmost: bool,
}

impl SurfaceProperties {
    pub const REQUIRED: Self = Self {
        click_through: true,
        never_activates: true,
        hidden_from_switcher: true,
        excluded_from_capture: true,
        topmost: true,
    };

    pub fn satisfies_requirements(&self) -> bool {
        *self == Self::REQUIRED
    }

    /// Names the properties that are missing, for a log line that says what is actually wrong.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.click_through {
            missing.push("click-through");
        }
        if !self.never_activates {
            missing.push("never activates");
        }
        if !self.hidden_from_switcher {
            missing.push("hidden from the switcher");
        }
        if !self.excluded_from_capture {
            missing.push("excluded from capture");
        }
        if !self.topmost {
            missing.push("topmost");
        }
        missing
    }
}

pub trait OverlaySurface {
    fn size(&self) -> (u32, u32);

    fn properties(&self) -> SurfaceProperties;

    fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfaceError>;

    /// Uploads the composited frame, touching only the damaged regions.
    fn present(&mut self, frame: &Frame, damage: &[Rect]) -> Result<(), SurfaceError>;

    /// Removes everything the overlay drew. Any pipeline failure ends here, so the host application
    /// is left exactly as it was.
    fn clear(&mut self) -> Result<(), SurfaceError>;
}

/// An in-memory surface. It records what was presented so tests can assert on damage without a
/// display, and the headless harness can write the result to disk.
#[derive(Debug)]
pub struct MemorySurface {
    width: u32,
    height: u32,
    frame: Frame,
    pub presented_damage: Vec<Vec<Rect>>,
    pub clears: usize,
}

impl MemorySurface {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            frame: Frame::filled(width, height, crate::compositor::CLEAR).expect("non-zero surface dimensions"),
            presented_damage: Vec::new(),
            clears: 0,
        }
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    pub fn presented_pixels(&self) -> u64 {
        self.presented_damage
            .iter()
            .flat_map(|damage| damage.iter())
            .map(Rect::area)
            .sum()
    }
}

impl OverlaySurface for MemorySurface {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn properties(&self) -> SurfaceProperties {
        SurfaceProperties::REQUIRED
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfaceError> {
        self.frame =
            Frame::filled(width, height, crate::compositor::CLEAR).map_err(|error| SurfaceError::Platform {
                operation: "resize",
                detail: error.to_string(),
            })?;
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn present(&mut self, frame: &Frame, damage: &[Rect]) -> Result<(), SurfaceError> {
        if frame.width() != self.width || frame.height() != self.height {
            return Err(SurfaceError::SizeMismatch {
                width: frame.width(),
                height: frame.height(),
                surface_width: self.width,
                surface_height: self.height,
            });
        }
        let bounds = self.frame.bounds();
        for region in damage {
            let Some(region) = region.clamp_to(&bounds) else {
                continue;
            };
            for y in region.y as u32..region.bottom() as u32 {
                for x in region.x as u32..region.right() as u32 {
                    self.frame.set_pixel(x, y, frame.pixel(x, y));
                }
            }
        }
        self.presented_damage.push(damage.to_vec());
        Ok(())
    }

    fn clear(&mut self) -> Result<(), SurfaceError> {
        self.frame = Frame::filled(self.width, self.height, crate::compositor::CLEAR).map_err(|error| {
            SurfaceError::Platform {
                operation: "clear",
                detail: error.to_string(),
            }
        })?;
        self.clears += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::{Compositor, OverlayBlock, OverlayLayout, OverlayStyle};

    #[test]
    fn the_required_properties_are_all_set_together() {
        assert!(SurfaceProperties::REQUIRED.satisfies_requirements());
        assert!(SurfaceProperties::REQUIRED.missing().is_empty());
    }

    #[test]
    fn a_missing_property_is_named() {
        let properties = SurfaceProperties {
            excluded_from_capture: false,
            ..SurfaceProperties::REQUIRED
        };
        assert!(!properties.satisfies_requirements());
        assert_eq!(properties.missing(), vec!["excluded from capture"]);
    }

    #[test]
    fn presenting_a_mismatched_frame_is_rejected() {
        let mut surface = MemorySurface::new(64, 64);
        let frame = Frame::filled(32, 32, [0, 0, 0, 0]).unwrap();
        let error = surface.present(&frame, &[Rect::new(0, 0, 32, 32)]).unwrap_err();
        assert!(matches!(error, SurfaceError::SizeMismatch { .. }));
    }

    #[test]
    fn only_damaged_pixels_are_copied() {
        let mut surface = MemorySurface::new(64, 64);
        let mut frame = Frame::filled(64, 64, [9, 9, 9, 255]).unwrap();
        frame.fill_rect(Rect::new(0, 0, 8, 8), [1, 2, 3, 255]);
        surface.present(&frame, &[Rect::new(0, 0, 8, 8)]).unwrap();
        assert_eq!(surface.frame().pixel(2, 2), [1, 2, 3, 255]);
        assert_eq!(surface.frame().pixel(40, 40), [0, 0, 0, 0]);
    }

    #[test]
    fn a_static_layout_presents_fewer_pixels_after_the_first_frame() {
        let mut compositor = Compositor::new();
        let mut surface = MemorySurface::new(640, 480);
        let frame = Frame::filled(640, 480, [30, 30, 30, 255]).unwrap();
        let layout = OverlayLayout::new(OverlayStyle::Seamless)
            .with_blocks(vec![OverlayBlock::new(Rect::new(20, 20, 200, 30), "Начать игру")]);

        let first = compositor.compose(&frame, &layout);
        surface.present(&first.frame, &first.damage).unwrap();
        let first_cost = surface.presented_pixels();

        let second = compositor.compose(&frame, &layout);
        surface.present(&second.frame, &second.damage).unwrap();

        assert!(surface.presented_pixels() - first_cost < first_cost / 10);
    }

    #[test]
    fn clearing_removes_everything_the_overlay_drew() {
        let mut surface = MemorySurface::new(32, 32);
        let frame = Frame::filled(32, 32, [7, 7, 7, 255]).unwrap();
        surface.present(&frame, &[Rect::new(0, 0, 32, 32)]).unwrap();
        surface.clear().unwrap();
        assert!(surface.frame().as_bytes().iter().all(|byte| *byte == 0));
        assert_eq!(surface.clears, 1);
    }

    #[test]
    fn resizing_replaces_the_surface_contents() {
        let mut surface = MemorySurface::new(32, 32);
        surface.resize(64, 48).unwrap();
        assert_eq!(surface.size(), (64, 48));
        assert!(surface.frame().as_bytes().iter().all(|byte| *byte == 0));
    }
}
