use crate::geometry::Rect;

/// A captured frame in straight (non-premultiplied) BGRA8, the layout every Windows capture path
/// already produces, so frames reach the pipeline without a conversion pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    stride: usize,
    pixels: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame dimensions must be non-zero, got {width}x{height}")]
    EmptyDimensions { width: u32, height: u32 },
    #[error("stride {stride} is smaller than one row of {width} BGRA pixels")]
    StrideTooSmall { stride: usize, width: u32 },
    #[error("buffer holds {actual} bytes, expected {expected} for {height} rows of stride {stride}")]
    BufferSize {
        actual: usize,
        expected: usize,
        height: u32,
        stride: usize,
    },
}

impl Frame {
    pub fn from_bgra(width: u32, height: u32, stride: usize, pixels: Vec<u8>) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::EmptyDimensions { width, height });
        }
        let row_bytes = width as usize * 4;
        if stride < row_bytes {
            return Err(FrameError::StrideTooSmall { stride, width });
        }
        let expected = stride * height as usize;
        if pixels.len() != expected {
            return Err(FrameError::BufferSize {
                actual: pixels.len(),
                expected,
                height,
                stride,
            });
        }
        Ok(Self {
            width,
            height,
            stride,
            pixels,
        })
    }

    pub fn packed(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, FrameError> {
        Self::from_bgra(width, height, width as usize * 4, pixels)
    }

    pub fn filled(width: u32, height: u32, bgra: [u8; 4]) -> Result<Self, FrameError> {
        let pixels = bgra
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 4)
            .collect();
        Self::packed(width, height, pixels)
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn stride(&self) -> usize {
        self.stride
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    pub fn row(&self, y: u32) -> &[u8] {
        let start = y as usize * self.stride;
        &self.pixels[start..start + self.width as usize * 4]
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let offset = y as usize * self.stride + x as usize * 4;
        [
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, bgra: [u8; 4]) {
        let offset = y as usize * self.stride + x as usize * 4;
        self.pixels[offset..offset + 4].copy_from_slice(&bgra);
    }

    pub fn fill_rect(&mut self, rect: Rect, bgra: [u8; 4]) {
        let Some(rect) = rect.clamp_to(&self.bounds()) else {
            return;
        };
        for y in rect.y as u32..rect.bottom() as u32 {
            for x in rect.x as u32..rect.right() as u32 {
                self.set_pixel(x, y, bgra);
            }
        }
    }

    /// Copies a region into a packed frame of its own, the form recognition engines expect.
    pub fn crop(&self, rect: Rect) -> Option<Frame> {
        let rect = rect.clamp_to(&self.bounds())?;
        let row_bytes = rect.width as usize * 4;
        let mut pixels = Vec::with_capacity(row_bytes * rect.height as usize);
        for y in rect.y as u32..rect.bottom() as u32 {
            let start = y as usize * self.stride + rect.x as usize * 4;
            pixels.extend_from_slice(&self.pixels[start..start + row_bytes]);
        }
        Frame::packed(rect.width, rect.height, pixels).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_pixel_frame_round_trips() {
        let mut frame = Frame::filled(1, 1, [1, 2, 3, 4]).unwrap();
        assert_eq!(frame.pixel(0, 0), [1, 2, 3, 4]);
        frame.set_pixel(0, 0, [9, 8, 7, 6]);
        assert_eq!(frame.pixel(0, 0), [9, 8, 7, 6]);
        assert_eq!(frame.row(0).len(), 4);
        assert_eq!(frame.crop(frame.bounds()).unwrap().as_bytes(), &[9, 8, 7, 6]);
    }

    #[test]
    fn zero_dimensions_are_rejected() {
        let error = Frame::filled(0, 4, [0, 0, 0, 255]).unwrap_err();
        assert!(matches!(error, FrameError::EmptyDimensions { width: 0, height: 4 }));
    }

    #[test]
    fn a_crop_of_a_padded_frame_drops_the_padding() {
        let mut frame = Frame::from_bgra(2, 2, 16, vec![7; 32]).unwrap();
        frame.set_pixel(0, 0, [1, 1, 1, 1]);
        frame.set_pixel(1, 0, [2, 2, 2, 2]);
        frame.set_pixel(0, 1, [3, 3, 3, 3]);
        frame.set_pixel(1, 1, [4, 4, 4, 4]);
        let cropped = frame.crop(frame.bounds()).unwrap();
        assert_eq!(cropped.stride(), 8);
        assert_eq!(cropped.as_bytes().len(), 16);
        assert_eq!(cropped.pixel(1, 1), [4, 4, 4, 4]);
        assert!(!cropped.as_bytes().contains(&7));
    }

    #[test]
    fn rejects_buffers_that_do_not_match_the_declared_geometry() {
        let error = Frame::packed(4, 4, vec![0; 16]).unwrap_err();
        assert!(matches!(error, FrameError::BufferSize { .. }));
    }

    #[test]
    fn rejects_stride_shorter_than_a_row() {
        let error = Frame::from_bgra(4, 2, 8, vec![0; 16]).unwrap_err();
        assert!(matches!(error, FrameError::StrideTooSmall { .. }));
    }

    #[test]
    fn padded_rows_are_addressed_through_the_stride() {
        let mut frame = Frame::from_bgra(2, 2, 16, vec![0; 32]).unwrap();
        frame.set_pixel(1, 1, [1, 2, 3, 4]);
        assert_eq!(frame.pixel(1, 1), [1, 2, 3, 4]);
        assert_eq!(frame.pixel(1, 0), [0, 0, 0, 0]);
        assert_eq!(frame.row(1).len(), 8);
    }

    #[test]
    fn crop_produces_a_packed_frame_of_the_requested_region() {
        let mut frame = Frame::filled(8, 8, [0, 0, 0, 255]).unwrap();
        frame.fill_rect(Rect::new(2, 2, 3, 3), [255, 255, 255, 255]);
        let cropped = frame.crop(Rect::new(2, 2, 3, 3)).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (3, 3));
        assert_eq!(cropped.stride(), 12);
        assert!(cropped.as_bytes().iter().all(|byte| *byte == 255));
    }

    #[test]
    fn crop_outside_the_frame_yields_nothing() {
        let frame = Frame::filled(8, 8, [0, 0, 0, 255]).unwrap();
        assert!(frame.crop(Rect::new(40, 40, 4, 4)).is_none());
    }

    #[test]
    fn fill_rect_clips_to_the_frame() {
        let mut frame = Frame::filled(4, 4, [0, 0, 0, 255]).unwrap();
        frame.fill_rect(Rect::new(-2, -2, 4, 4), [9, 9, 9, 255]);
        assert_eq!(frame.pixel(0, 0), [9, 9, 9, 255]);
        assert_eq!(frame.pixel(2, 2), [0, 0, 0, 255]);
    }
}
