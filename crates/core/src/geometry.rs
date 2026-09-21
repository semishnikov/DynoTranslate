use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right() && other.x < self.right() && self.y < other.bottom() && other.y < self.bottom()
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            return None;
        }
        Some(Rect::new(x, y, (right - x) as u32, (bottom - y) as u32))
    }

    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect::new(x, y, (right - x) as u32, (bottom - y) as u32)
    }

    /// Ratio of the intersection to the union, used to match text blocks across frames.
    pub fn iou(&self, other: &Rect) -> f32 {
        let intersection = match self.intersection(other) {
            Some(rect) => rect.area(),
            None => return 0.0,
        };
        let union = self.area() + other.area() - intersection;
        if union == 0 {
            return 0.0;
        }
        intersection as f32 / union as f32
    }

    pub fn inflate(&self, by: u32) -> Rect {
        let by_i = by as i32;
        Rect::new(self.x - by_i, self.y - by_i, self.width + by * 2, self.height + by * 2)
    }

    pub fn clamp_to(&self, bounds: &Rect) -> Option<Rect> {
        self.intersection(bounds)
    }
}

/// Merges overlapping rectangles so a presenter never updates the same pixels twice in one frame.
///
/// Rectangles that merely touch are also merged: presenting two adjacent regions costs more than
/// presenting their union, and the union of touching rectangles wastes no pixels.
pub fn coalesce(mut rects: Vec<Rect>) -> Vec<Rect> {
    rects.retain(|rect| !rect.is_empty());
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                if rects[i].inflate(1).intersects(&rects[j]) {
                    let union = rects[i].union(&rects[j]);
                    rects.swap_remove(j);
                    rects[i] = union;
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
    rects.sort_by_key(|rect| (rect.y, rect.x));
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_of_disjoint_rects_is_none() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(20, 20, 5, 5);
        assert_eq!(a.intersection(&b), None);
        assert_eq!(a.iou(&b), 0.0);
    }

    #[test]
    fn identical_rects_have_iou_of_one() {
        let a = Rect::new(4, 8, 120, 30);
        assert!((a.iou(&a) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn half_overlap_has_expected_iou() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 0, 10, 10);
        assert!((a.iou(&b) - (50.0 / 150.0)).abs() < 1e-6);
    }

    #[test]
    fn union_ignores_empty_rects() {
        let a = Rect::new(0, 0, 0, 0);
        let b = Rect::new(3, 3, 4, 4);
        assert_eq!(a.union(&b), b);
        assert_eq!(b.union(&a), b);
    }

    #[test]
    fn coalesce_merges_touching_rects() {
        let rects = vec![Rect::new(0, 0, 10, 10), Rect::new(10, 0, 10, 10), Rect::new(60, 60, 5, 5)];
        let merged = coalesce(rects);
        assert_eq!(merged, vec![Rect::new(0, 0, 20, 10), Rect::new(60, 60, 5, 5)]);
    }

    #[test]
    fn coalesce_drops_empty_rects() {
        let merged = coalesce(vec![Rect::new(0, 0, 0, 12), Rect::new(2, 2, 3, 3)]);
        assert_eq!(merged, vec![Rect::new(2, 2, 3, 3)]);
    }

    #[test]
    fn clamping_keeps_rects_inside_the_frame() {
        let frame = Rect::new(0, 0, 100, 100);
        assert_eq!(Rect::new(-10, -10, 30, 30).clamp_to(&frame), Some(Rect::new(0, 0, 20, 20)));
        assert_eq!(Rect::new(200, 0, 10, 10).clamp_to(&frame), None);
    }
}
