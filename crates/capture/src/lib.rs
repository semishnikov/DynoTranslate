//! Capture sources.
//!
//! Every source implements [`CaptureSource`]. The pipeline, the tests and the headless harness only
//! ever see that trait, so a Windows capture session and a generated scene are interchangeable.

use lumen_core::{Frame, Rect};

pub mod synthetic;

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::{capture_window_picture, capture_window_screen, enumerate_targets, DesktopCopySource};

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("the target window is gone")]
    TargetLost,
    #[error("the target has no visible pixels")]
    TargetNotVisible,
    #[error("no frame was available before the deadline")]
    Timeout,
    #[error("the capture device was reset and the session must be recreated")]
    DeviceLost,
    #[error("the frame the source produced is unusable: {0}")]
    MalformedFrame(#[from] lumen_core::FrameError),
    #[error("{operation} failed: {detail}")]
    Platform { operation: &'static str, detail: String },
}

/// Identifies what is being captured. Windows handles never leave the platform adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureTarget {
    pub id: u64,
    pub title: String,
    pub process: String,
    pub bounds: Rect,
}

pub trait CaptureSource {
    fn target(&self) -> &CaptureTarget;

    /// Returns the next frame, or `Ok(None)` when the source has nothing new to offer.
    fn next_frame(&mut self) -> Result<Option<Frame>, CaptureError>;
}
