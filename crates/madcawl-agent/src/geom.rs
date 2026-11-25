//! Where each node is on the canvas, from the fragment tree: the union
//! of its fragments, and whether any of them is shown — has area and is
//! not clipped away by an `overflow: hidden` or `clip` ancestor or put
//! off the canvas.

use std::collections::HashMap;
use std::sync::Arc;

use madcawl_css::style::{Overflow, Styles};
use madcawl_dom::NodeId;
use madcawl_layout::{FragKind, Fragment, Layout, Point};

/// A rectangle in CSS px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect {
            x,
            y,
            w: (self.x + self.w).max(o.x + o.w) - x,
            h: (self.y + self.h).max(o.y + o.h) - y,
        }
    }

    fn intersect(self, o: Rect) -> Option<Rect> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = (self.x + self.w).min(o.x + o.w);
        let b = (self.y + self.h).min(o.y + o.h);
        (r > x && b > y).then_some(Rect {
            x,
            y,
            w: r - x,
            h: b - y,
        })
    }

    pub fn intersects(self, o: Rect) -> bool {
        self.intersect(o).is_some()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub rect: Rect,
    /// Some fragment of it has area and is not clipped away
    pub shown: bool,
}

/// The places of a laid-out document's nodes, and its viewport.
pub struct Geometry {
    places: HashMap<NodeId, Place>,
    pub viewport: Rect,
    version: u64,
}

impl Geometry {
    /// Lays the document out if it is not, and reads every fragment.
    pub fn from_layout(layout: &mut Layout, styles: &Styles, width: f32, height: f32) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let mut g = Geometry {
            version: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            places: HashMap::new(),
            viewport: Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
            },
        };
        if let Some(root) = layout.fragments() {
            // what is left of or above the origin is off the canvas:
            // nothing scrolls to it
            let canvas = Rect {
                x: 0.0,
                y: 0.0,
                w: f32::MAX / 4.0,
                h: f32::MAX / 4.0,
            };
            g.walk(styles, &root.frag, root.offset, canvas);
        }
        g
    }

    /// An explicit stack, so a deep fragment tree cannot overflow the
    /// call stack.
    fn walk(&mut self, styles: &Styles, root: &Arc<Fragment>, at: Point, canvas: Rect) {
        let mut stack = vec![(root.clone(), at, canvas)];
        while let Some((f, at, clip)) = stack.pop() {
            let r = Rect {
                x: at.x.px(),
                y: at.y.px(),
                w: f.size.w.px(),
                h: f.size.h.px(),
            };
            let painted = match f.kind {
                FragKind::Text => r,
                _ => Rect {
                    x: r.x + f.overflow.x.px(),
                    y: r.y + f.overflow.y.px(),
                    w: f.overflow.w.px(),
                    h: f.overflow.h.px(),
                },
            };
            let shown = painted.intersect(clip).is_some();
            if let Some(n) = f.node.filter(|_| f.kind != FragKind::Line) {
                self.places
                    .entry(n)
                    .and_modify(|p| {
                        p.rect = p.rect.union(r);
                        p.shown |= shown;
                    })
                    .or_insert(Place { rect: r, shown });
            }
            let inner = match f.node.and_then(|n| styles.get(n)) {
                Some(s) if f.scroller && clips(s.place.overflow_x, s.place.overflow_y) => {
                    clip.intersect(r).unwrap_or_default()
                }
                _ => clip,
            };
            for c in f.children.iter().rev() {
                stack.push((c.frag.clone(), at + c.offset, inner));
            }
        }
    }

    /// Different for every geometry read in this process.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn place(&self, n: NodeId) -> Option<Place> {
        self.places.get(&n).copied()
    }

    /// Some part of the node is in the viewport; a node without a place
    /// is taken to be where its parent is.
    pub fn in_viewport(&self, n: NodeId) -> Option<bool> {
        self.place(n)
            .map(|p| p.shown && p.rect.intersects(self.viewport))
    }

    pub fn bytes(&self) -> usize {
        self.places.capacity() * (size_of::<NodeId>() + size_of::<Place>() + 8)
    }
}

/// A scroll container whose overflow is hidden or clipped: what is
/// outside it is not seen (one axis clipping is taken as both).
fn clips(x: Overflow, y: Overflow) -> bool {
    matches!(x, Overflow::Hidden | Overflow::Clip) || matches!(y, Overflow::Hidden | Overflow::Clip)
}
