//! Floats: the floats of a block formatting context, each placed as high as it fits and as far to its side
//! as it can, and the room they leave for line boxes and for boxes that start a new formatting context.
//! Coordinates are the formatting context root's border box's.

use std::cell::Cell;

use madcawl_css::style::Clear;

use crate::au::{Au, Point, Rect};

/// The floats of one block formatting context, bounded by the boxes of its document.
pub struct Exclusions {
    floats: Vec<(bool, Rect)>,
    /// The floats before this index all end at or above the second value: a query at or below it skips
    /// them, which keeps a long run of lines beside many small floats linear
    skip: Cell<(usize, Au)>,
    /// The highest top, lowest bottom, and lowest bottom of the left and of the right floats
    top: Au,
    bottom: Au,
    side_bottom: [Au; 2],
    visits: Cell<u64>,
}

impl Default for Exclusions {
    fn default() -> Exclusions {
        Exclusions {
            floats: Vec::new(),
            skip: Cell::new((0, Au::MIN)),
            top: Au::MIN,
            bottom: Au::ZERO,
            side_bottom: [Au::MIN; 2],
            visits: Cell::new(0),
        }
    }
}

fn overlaps(r: &Rect, y: Au, h: Au) -> bool {
    let h = h.max(Au(1));
    r.y < y + h && r.y + r.h > y && r.h > Au::ZERO
}

impl Exclusions {
    pub fn is_empty(&self) -> bool {
        self.floats.is_empty()
    }

    pub fn len(&self) -> usize {
        self.floats.len()
    }

    /// Float entries looked at so far.
    pub fn visits(&self) -> u64 {
        self.visits.get()
    }

    // the floats a query at `y` or below must look at
    fn from(&self, y: Au) -> impl Iterator<Item = &(bool, Rect)> {
        let (mut k, mut end) = self.skip.get();
        if y < end {
            (k, end) = (0, Au::MIN);
        }
        let skipped = k;
        while let Some((_, f)) = self.floats.get(k)
            && f.y + f.h <= y
        {
            end = end.max(f.y + f.h);
            k += 1;
        }
        self.skip.set((k, end));
        self.visits.set(self.visits.get() + (k - skipped) as u64);
        self.floats[k..]
            .iter()
            .inspect(|_| self.visits.set(self.visits.get() + 1))
    }

    /// The room between the floats beside the band from `y` down `h`, within `left`..`right`.
    pub fn band(&self, y: Au, h: Au, left: Au, right: Au) -> (Au, Au) {
        let (mut l, mut r) = (left, right);
        for (is_left, f) in self.from(y) {
            if f.y >= y + h.max(Au(1)) {
                break;
            }
            if !overlaps(f, y, h) {
                continue;
            }
            if *is_left {
                l = l.max(f.x + f.w);
            } else {
                r = r.min(f.x);
            }
        }
        (l, r)
    }

    fn push(&mut self, is_left: bool, r: Rect) -> Point {
        self.top = self.top.max(r.y);
        self.bottom = self.bottom.max(r.y + r.h);
        let side = &mut self.side_bottom[usize::from(!is_left)];
        *side = (*side).max(r.y + r.h);
        self.floats.push((is_left, r));
        Point { x: r.x, y: r.y }
    }

    /// Where the margin box of `w` by `h` floated to the left (or right) goes, no higher than `y`, within
    /// `left`..`right`; it is added.
    pub fn place(&mut self, is_left: bool, w: Au, h: Au, y: Au, left: Au, right: Au) -> Point {
        // a float is no higher than any earlier one
        let mut y = y.max(self.top);
        loop {
            let (l, r) = self.band(y, h, left, right);
            let blocked = self
                .from(y)
                .take_while(|(_, f)| f.y < y + h.max(Au(1)))
                .any(|(_, f)| overlaps(f, y, h));
            if r - l >= w || !blocked {
                let x = if is_left { l } else { r - w };
                return self.push(is_left, Rect { x, y, w, h });
            }
            // down to the next float bottom below `y`
            match self.next_bottom(y) {
                Some(n) => y = n,
                None => {
                    let x = if is_left { l } else { r - w };
                    return self.push(is_left, Rect { x, y, w, h });
                }
            }
        }
    }

    /// The lowest bottom of the floats `clear` names, if below `y`.
    pub fn clear(&self, y: Au, clear: Clear) -> Au {
        let [l, r] = self.side_bottom;
        match clear {
            Clear::None => y,
            Clear::Left | Clear::InlineStart => y.max(l),
            Clear::Right | Clear::InlineEnd => y.max(r),
            Clear::Both => y.max(l).max(r),
        }
    }

    /// The bottom of the lowest float.
    pub fn bottom(&self) -> Au {
        self.bottom
    }

    /// The next float bottom below `y`, where a line that did not fit beside them may fit.
    pub fn next_bottom(&self, y: Au) -> Option<Au> {
        self.from(y)
            .map(|(_, f)| f.y + f.h)
            .filter(|&b| b > y)
            .min()
    }
}
