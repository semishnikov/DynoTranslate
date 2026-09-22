//! Overlay composition and presentation.
//!
//! The compositor is deterministic and runs anywhere: the same layout always produces the same
//! pixels, so regressions in erasure, plating and damage tracking are caught by golden images on a
//! build agent. Presentation is a thin platform contract on top of it.

pub mod compositor;
pub mod surface;

pub use compositor::{Compositor, OverlayBlock, OverlayLayout, OverlayStyle};
pub use lumen_render::{FontWeight, TextAlign, WritingMode};
pub use surface::{OverlaySurface, SurfaceError, SurfaceProperties};

#[cfg(windows)]
pub mod windows;
