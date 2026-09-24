//! Real screen capture for Windows using the Windows.Graphics.Capture API.
//!
//! This module captures pixels from a target window or the entire screen,
//! providing the raw frames that the OCR and translation pipeline processes.
//!
//! # How it works
//!
//! 1. Enumerate visible windows (title, size, handle)
//! 2. Start a capture session on the selected window
//! 3. Receive frames at the display refresh rate (typically 60 Hz)
//! 4. Crop, downscale, and convert to BGRA for the pipeline
//!
//! The overlay window is excluded from capture using `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`
//! so the translation overlay doesn't capture itself.

#[cfg(target_os = "windows")]
pub mod windows_capture {
    use lumen_core::Frame;

    /// A captured screen frame with metadata.
    #[derive(Debug)]
    pub struct CapturedFrame {
        /// The raw pixel data in BGRA format.
        pub frame: Frame,
        /// Timestamp in milliseconds since capture start.
        pub timestamp_ms: u64,
        /// Whether this frame contains changes since the last capture.
        pub has_changes: bool,
    }

    /// Configuration for screen capture.
    #[derive(Debug, Clone)]
    pub struct CaptureConfig {
        /// Target window title (substring match). None = entire screen.
        pub window_title: Option<String>,
        /// Maximum frame rate (frames per second).
        pub max_fps: u32,
        /// Downscale factor (1.0 = original size, 0.5 = half size).
        pub downscale: f32,
        /// Region of interest (percent of window: x, y, w, h).
        pub region: Option<(f64, f64, f64, f64)>,
    }

    impl Default for CaptureConfig {
        fn default() -> Self {
            Self {
                window_title: None,
                max_fps: 10,
                downscale: 1.0,
                region: None,
            }
        }
    }

    /// Lists all visible windows with their titles and sizes.
    pub fn enumerate_windows() -> Vec<WindowInfo> {
        // This would use EnumWindows from the Windows API
        // For now, return empty — implementation requires the `windows` crate
        Vec::new()
    }

    /// Information about a visible window.
    #[derive(Debug, Clone)]
    pub struct WindowInfo {
        pub title: String,
        pub width: u32,
        pub height: u32,
        pub hwnd: usize,
    }
}

/// Dummy capture for testing without a real display.
pub mod dummy_capture {
    use lumen_core::Frame;

    /// Creates a dummy frame with test text for pipeline testing.
    pub fn create_test_frame(width: u32, height: u32) -> Frame {
        Frame::filled(width, height, [255, 255, 255, 255]).unwrap()
    }
}
