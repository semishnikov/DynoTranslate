//! Erasing the original text by local inpainting.
//!
//! The overlay covers the source box with pixels reconstructed from the ring just outside it,
//! so a translation sitting on a gradient or a textured panel looks like it belongs there
//! instead of sitting on a flat patch. The solver is a fixed-count Jacobi relaxation of the
//! Dirichlet problem: boundary pixels are held at their source values, the interior starts at
//! their mean and is averaged with its four neighbours until the iteration budget runs out.
//! The budget is a constant, not an epsilon exit, so the same region always produces the same
//! bytes.

use lumen_core::{Frame, Rect};

/// Relaxation steps. Enough for a tooltip-sized box to settle visually; large boxes keep a
/// soft blur rather than a flat fill, which is the point of inpainting over a solid colour.
const ITERATIONS: usize = 48;

/// Reconstructs the pixels of `region` from the frame around it.
///
/// Returns the inpainted pixels in row-major order, packed as BGRA. A region that leaves the
/// frame is clamped; a region with no pixels inside the frame yields an empty vector.
pub fn inpaint(source: &Frame, region: Rect) -> Vec<[u8; 4]> {
    let Some(region) = region.clamp_to(&source.bounds()) else {
        return Vec::new();
    };
    let width = region.width as usize;
    let height = region.height as usize;
    if width == 0 || height == 0 {
        return Vec::new();
    }

    // One-pixel halo of real source pixels is the Dirichlet boundary.
    let halo = Rect::new(region.x - 1, region.y - 1, region.width + 2, region.height + 2)
        .clamp_to(&source.bounds());
    let halo = match halo {
        Some(halo) => halo,
        None => return vec![[0, 0, 0, 0]; width * height],
    };

    // Working grid includes the halo where the frame has it; edges that fall outside the
    // frame reuse the nearest in-frame pixel so a box flush with the window edge still has
    // a boundary.
    let grid_w = width + 2;
    let grid_h = height + 2;
    let mut grid = vec![[0.0_f32; 4]; grid_w * grid_h];
    let mut fixed = vec![false; grid_w * grid_h];

    let mut sum = [0.0_f32; 4];
    let mut count = 0_u32;
    for gy in 0..grid_h {
        for gx in 0..grid_w {
            let sx = (region.x - 1 + gx as i32).clamp(halo.x, halo.right() - 1);
            let sy = (region.y - 1 + gy as i32).clamp(halo.y, halo.bottom() - 1);
            let pixel = source.pixel(sx as u32, sy as u32);
            let on_boundary = gx == 0 || gy == 0 || gx == grid_w - 1 || gy == grid_h - 1;
            let index = gy * grid_w + gx;
            if on_boundary {
                grid[index] = [
                    pixel[0] as f32,
                    pixel[1] as f32,
                    pixel[2] as f32,
                    pixel[3] as f32,
                ];
                fixed[index] = true;
                sum[0] += grid[index][0];
                sum[1] += grid[index][1];
                sum[2] += grid[index][2];
                sum[3] += grid[index][3];
                count += 1;
            }
        }
    }

    let seed = if count == 0 {
        [0.0; 4]
    } else {
        [
            sum[0] / count as f32,
            sum[1] / count as f32,
            sum[2] / count as f32,
            sum[3] / count as f32,
        ]
    };
    for (index, cell) in grid.iter_mut().enumerate() {
        if !fixed[index] {
            *cell = seed;
        }
    }

    let mut scratch = grid.clone();
    for _ in 0..ITERATIONS {
        for y in 1..grid_h - 1 {
            for x in 1..grid_w - 1 {
                let index = y * grid_w + x;
                if fixed[index] {
                    continue;
                }
                let mut acc = [0.0_f32; 4];
                for (dx, dy) in [(-1_isize, 0), (1, 0), (0, -1), (0, 1)] {
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;
                    let neighbour = ny as usize * grid_w + nx as usize;
                    for channel in 0..4 {
                        acc[channel] += grid[neighbour][channel];
                    }
                }
                for channel in 0..4 {
                    scratch[index][channel] = acc[channel] * 0.25;
                }
            }
        }
        std::mem::swap(&mut grid, &mut scratch);
    }

    let mut out = Vec::with_capacity(width * height);
    for y in 1..grid_h - 1 {
        for x in 1..grid_w - 1 {
            let index = y * grid_w + x;
            let cell = grid[index];
            out.push([
                cell[0].round().clamp(0.0, 255.0) as u8,
                cell[1].round().clamp(0.0, 255.0) as u8,
                cell[2].round().clamp(0.0, 255.0) as u8,
                cell[3].round().clamp(0.0, 255.0) as u8,
            ]);
        }
    }
    out
}

/// Convenience: write [`inpaint`] directly into `destination` at `region`'s position.
pub fn erase(source: &Frame, destination: &mut Frame, region: Rect) {
    let pixels = inpaint(source, region);
    let Some(region) = region.clamp_to(&destination.bounds()) else {
        return;
    };
    let width = region.width as usize;
    if width == 0 {
        return;
    }
    for (i, pixel) in pixels.iter().enumerate() {
        let x = region.x + (i % width) as i32;
        let y = region.y + (i / width) as i32;
        if x >= 0 && y >= 0 && x < destination.width() as i32 && y < destination.height() as i32 {
            destination.set_pixel(x as u32, y as u32, *pixel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_with_gradient(width: u32, height: u32) -> Frame {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..height {
            for x in 0..width {
                let shade = ((x * 255) / width.max(1)) as u8;
                pixels.extend_from_slice(&[shade, shade, shade, 255]);
            }
        }
        Frame::packed(width, height, pixels).expect("dimensions")
    }

    #[test]
    fn a_region_outside_the_frame_yields_nothing() {
        let frame = frame_with_gradient(16, 16);
        assert!(inpaint(&frame, Rect::new(100, 100, 4, 4)).is_empty());
    }

    #[test]
    fn a_flat_surface_stays_flat() {
        let frame = Frame::filled(32, 32, [40, 40, 40, 255]).expect("dimensions");
        let pixels = inpaint(&frame, Rect::new(8, 8, 12, 8));
        assert_eq!(pixels.len(), 12 * 8);
        assert!(pixels.iter().all(|p| *p == [40, 40, 40, 255]));
    }

    #[test]
    fn a_horizontal_gradient_is_reproduced_not_flattened() {
        let frame = frame_with_gradient(64, 16);
        // In the middle rows the boundary left/right values drive the interior.
        let pixels = inpaint(&frame, Rect::new(16, 4, 16, 8));
        let first = pixels[4]; // x=16 row interior start approx
        let last = pixels[16 * 4 + 15];
        assert!(
            last[0] > first[0],
            "expected left-to-right growth, got {first:?} then {last:?}"
        );
    }

    #[test]
    fn inpainting_is_deterministic() {
        let frame = frame_with_gradient(48, 48);
        let region = Rect::new(10, 10, 20, 14);
        assert_eq!(inpaint(&frame, region), inpaint(&frame, region));
    }

    #[test]
    fn erase_writes_the_reconstruction_into_the_destination() {
        let source = Frame::filled(20, 20, [10, 20, 30, 255]).expect("dimensions");
        let mut destination = Frame::filled(20, 20, [0, 0, 0, 0]).expect("dimensions");
        erase(&source, &mut destination, Rect::new(4, 4, 6, 6));
        assert_eq!(destination.pixel(6, 6), [10, 20, 30, 255]);
        assert_eq!(destination.pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn a_region_flush_with_the_edge_still_produces_pixels() {
        let frame = frame_with_gradient(16, 16);
        let pixels = inpaint(&frame, Rect::new(0, 0, 8, 8));
        assert_eq!(pixels.len(), 64);
    }
}
