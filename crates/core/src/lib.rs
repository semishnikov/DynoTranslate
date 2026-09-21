//! Platform-independent foundations of the Lumen pipeline.
//!
//! Everything here runs and is tested without a display server, which is what allows the capture,
//! scheduling and region math to be verified on Linux build agents while the Windows adapters are
//! compiled and exercised separately.

pub mod change;
pub mod frame;
pub mod geometry;
pub mod schedule;

pub use change::{ChangeDetector, ChangeReport, DEFAULT_TILE_SIZE};
pub use frame::{Frame, FrameError};
pub use geometry::{coalesce, Rect};
pub use schedule::{CaptureScheduler, Responsiveness, SchedulerConfig};
