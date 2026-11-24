//! Layout lengths: 1/64 px in an `i32`, the unit computed lengths use, with every operation saturating so no
//! page can overflow it.

use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// A length in 1/64 CSS px.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Au(pub i32);

pub const UNIT: i32 = 64;

impl Au {
    pub const ZERO: Au = Au(0);
    pub const MAX: Au = Au(i32::MAX);
    pub const MIN: Au = Au(i32::MIN);

    pub fn from_px(v: f32) -> Au {
        Au((v * UNIT as f32)
            .round()
            .clamp(i32::MIN as f32, i32::MAX as f32) as i32)
    }

    /// Truncated toward zero, as WebKit's layout units take a float.
    pub fn from_px_trunc(v: f32) -> Au {
        Au((v * UNIT as f32).clamp(i32::MIN as f32, i32::MAX as f32) as i32)
    }

    pub fn px(self) -> f32 {
        self.0 as f32 / UNIT as f32
    }

    pub fn scale(self, k: f32) -> Au {
        Au::from_px(self.px() * k)
    }

    pub fn max(self, o: Au) -> Au {
        Au(self.0.max(o.0))
    }

    pub fn min(self, o: Au) -> Au {
        Au(self.0.min(o.0))
    }

    /// Clamped to at least `lo`, then at most `hi`; `lo` wins when they cross, as `min-width` beats
    /// `max-width`.
    pub fn clamp_lo_hi(self, lo: Au, hi: Au) -> Au {
        self.min(hi).max(lo)
    }

    pub fn half(self) -> Au {
        Au(self.0 / 2)
    }

    /// Rounded to the nearest whole px.
    pub fn round_px(self) -> Au {
        Au::from_px(self.px().round())
    }
}

impl Add for Au {
    type Output = Au;
    fn add(self, o: Au) -> Au {
        Au(self.0.saturating_add(o.0))
    }
}

impl Sub for Au {
    type Output = Au;
    fn sub(self, o: Au) -> Au {
        Au(self.0.saturating_sub(o.0))
    }
}

impl Neg for Au {
    type Output = Au;
    fn neg(self) -> Au {
        Au(self.0.saturating_neg())
    }
}

impl AddAssign for Au {
    fn add_assign(&mut self, o: Au) {
        *self = *self + o;
    }
}

impl SubAssign for Au {
    fn sub_assign(&mut self, o: Au) {
        *self = *self - o;
    }
}

/// A point or offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: Au,
    pub y: Au,
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point {
            x: self.x + o.x,
            y: self.y + o.y,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Size {
    pub w: Au,
    pub h: Au,
}

/// A rectangle: its origin and size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: Au,
    pub y: Au,
    pub w: Au,
    pub h: Au,
}

impl Rect {
    /// The smallest rectangle holding both.
    pub fn union(self, o: Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        let r = (self.x + self.w).max(o.x + o.w);
        let b = (self.y + self.h).max(o.y + o.h);
        Rect {
            x,
            y,
            w: r - x,
            h: b - y,
        }
    }
}
