use crate::frame::Frame;
use crate::geometry::{coalesce, Rect};

pub const DEFAULT_TILE_SIZE: u32 = 64;

/// Splits frames into a fixed tile grid and reports which tiles changed since the previous frame.
///
/// Recognition is the expensive stage of the pipeline, so a static screen must cost nothing. Tiles
/// are compared by hash rather than by pixel equality: the hash is computed once per frame and
/// keeps the comparison independent of the tile's pixel count.
#[derive(Debug, Clone)]
pub struct ChangeDetector {
    tile_size: u32,
    grid: Option<Grid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Grid {
    width: u32,
    height: u32,
    hashes: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeReport {
    /// Tile rectangles that differ from the previous frame, coalesced into larger regions.
    pub regions: Vec<Rect>,
    pub changed_tiles: usize,
    pub total_tiles: usize,
    /// True when the frame geometry changed, which invalidates every cached tile.
    pub reset: bool,
}

impl ChangeReport {
    pub fn is_static(&self) -> bool {
        self.changed_tiles == 0
    }

    pub fn changed_fraction(&self) -> f32 {
        if self.total_tiles == 0 {
            return 0.0;
        }
        self.changed_tiles as f32 / self.total_tiles as f32
    }
}

impl ChangeDetector {
    pub fn new(tile_size: u32) -> Self {
        Self {
            tile_size: tile_size.max(8),
            grid: None,
        }
    }

    pub fn forget(&mut self) {
        self.grid = None;
    }

    pub fn accept(&mut self, frame: &Frame) -> ChangeReport {
        let columns = frame.width().div_ceil(self.tile_size);
        let rows = frame.height().div_ceil(self.tile_size);
        let total_tiles = (columns * rows) as usize;

        let reset = match &self.grid {
            Some(grid) => grid.width != frame.width() || grid.height != frame.height(),
            None => true,
        };

        let mut hashes = Vec::with_capacity(total_tiles);
        let mut changed = Vec::new();

        for row in 0..rows {
            for column in 0..columns {
                let rect = self.tile_rect(frame, column, row);
                let hash = hash_tile(frame, rect);
                let previous = if reset {
                    None
                } else {
                    self.grid.as_ref().and_then(|grid| grid.hashes.get((row * columns + column) as usize).copied())
                };
                if previous != Some(hash) {
                    changed.push(rect);
                }
                hashes.push(hash);
            }
        }

        self.grid = Some(Grid {
            width: frame.width(),
            height: frame.height(),
            hashes,
        });

        ChangeReport {
            changed_tiles: changed.len(),
            regions: coalesce(changed),
            total_tiles,
            reset,
        }
    }

    fn tile_rect(&self, frame: &Frame, column: u32, row: u32) -> Rect {
        let x = column * self.tile_size;
        let y = row * self.tile_size;
        Rect::new(
            x as i32,
            y as i32,
            self.tile_size.min(frame.width() - x),
            self.tile_size.min(frame.height() - y),
        )
    }
}

impl Default for ChangeDetector {
    fn default() -> Self {
        Self::new(DEFAULT_TILE_SIZE)
    }
}

/// FNV-1a over the tile's rows. Collisions would drop a repaint rather than corrupt anything, and
/// at 64 bits the probability is far below the rate at which frames are resampled anyway.
fn hash_tile(frame: &Frame, rect: Rect) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x1000_0000_01b3;

    let mut hash = OFFSET;
    let start = rect.x as usize * 4;
    let end = start + rect.width as usize * 4;
    for y in rect.y as u32..rect.bottom() as u32 {
        for byte in &frame.row(y)[start..end] {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame::filled(256, 128, [16, 16, 16, 255]).unwrap()
    }

    #[test]
    fn the_first_frame_is_entirely_new() {
        let mut detector = ChangeDetector::new(64);
        let report = detector.accept(&frame());
        assert!(report.reset);
        assert_eq!(report.total_tiles, 8);
        assert_eq!(report.changed_tiles, 8);
    }

    #[test]
    fn an_unchanged_frame_costs_no_regions() {
        let mut detector = ChangeDetector::new(64);
        let frame = frame();
        detector.accept(&frame);
        let report = detector.accept(&frame);
        assert!(report.is_static());
        assert!(report.regions.is_empty());
        assert_eq!(report.changed_fraction(), 0.0);
    }

    #[test]
    fn only_the_touched_tile_is_reported() {
        let mut detector = ChangeDetector::new(64);
        let mut frame = frame();
        detector.accept(&frame);
        frame.set_pixel(70, 70, [255, 255, 255, 255]);
        let report = detector.accept(&frame);
        assert_eq!(report.changed_tiles, 1);
        assert_eq!(report.regions, vec![Rect::new(64, 64, 64, 64)]);
    }

    #[test]
    fn adjacent_changed_tiles_are_coalesced() {
        let mut detector = ChangeDetector::new(64);
        let mut frame = frame();
        detector.accept(&frame);
        frame.fill_rect(Rect::new(60, 10, 80, 20), [200, 200, 200, 255]);
        let report = detector.accept(&frame);
        assert_eq!(report.changed_tiles, 3);
        assert_eq!(report.regions, vec![Rect::new(0, 0, 192, 64)]);
    }

    #[test]
    fn a_resize_invalidates_the_grid() {
        let mut detector = ChangeDetector::new(64);
        detector.accept(&frame());
        let report = detector.accept(&Frame::filled(128, 128, [16, 16, 16, 255]).unwrap());
        assert!(report.reset);
        assert_eq!(report.changed_tiles, report.total_tiles);
    }

    #[test]
    fn partial_edge_tiles_stay_inside_the_frame() {
        let mut detector = ChangeDetector::new(64);
        let report = detector.accept(&Frame::filled(100, 70, [0, 0, 0, 255]).unwrap());
        assert_eq!(report.total_tiles, 4);
        for region in &report.regions {
            assert!(region.right() <= 100 && region.bottom() <= 70);
        }
    }

    #[test]
    fn forgetting_the_grid_reports_everything_again() {
        let mut detector = ChangeDetector::new(64);
        let frame = frame();
        detector.accept(&frame);
        detector.forget();
        assert!(detector.accept(&frame).reset);
    }
}
