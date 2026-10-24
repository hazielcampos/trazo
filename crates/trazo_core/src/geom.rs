//! Small integer geometry helpers used by the raster engine.

use serde::{Deserialize, Serialize};

/// Integer rectangle, `x0..x1` × `y0..y1` (max edges exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    pub const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn from_size(w: u32, h: u32) -> Self {
        Self::new(0, 0, w as i32, h as i32)
    }

    /// Bounding box of a circle-ish area around a float point.
    pub fn around(cx: f32, cy: f32, radius: f32) -> Self {
        let r = radius.max(0.5) + 1.0;
        Self::new(
            (cx - r).floor() as i32,
            (cy - r).floor() as i32,
            (cx + r).ceil() as i32 + 1,
            (cy + r).ceil() as i32 + 1,
        )
    }

    pub fn width(&self) -> i32 {
        (self.x1 - self.x0).max(0)
    }

    pub fn height(&self) -> i32 {
        (self.y1 - self.y0).max(0)
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn union(&self, o: &IRect) -> IRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        IRect::new(
            self.x0.min(o.x0),
            self.y0.min(o.y0),
            self.x1.max(o.x1),
            self.y1.max(o.y1),
        )
    }

    pub fn intersect(&self, o: &IRect) -> IRect {
        let r = IRect::new(
            self.x0.max(o.x0),
            self.y0.max(o.y0),
            self.x1.min(o.x1),
            self.y1.min(o.y1),
        );
        if r.is_empty() {
            IRect::EMPTY
        } else {
            r
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_and_intersect() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 20, 20);
        assert_eq!(a.union(&b), IRect::new(0, 0, 20, 20));
        assert_eq!(a.intersect(&b), IRect::new(5, 5, 10, 10));
        assert!(a.intersect(&IRect::new(30, 30, 40, 40)).is_empty());
        assert_eq!(IRect::EMPTY.union(&a), a);
    }
}
