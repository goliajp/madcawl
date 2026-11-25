//! The fragment tree: what layout produces. A fragment is immutable and shared; it knows its own size and
//! where each child sits relative to its own top left corner, never its own position, so an unchanged child
//! can hang under a new parent as it is.

use std::sync::Arc;

use madcawl_dom::NodeId;

use crate::au::{Au, Point, Rect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragKind {
    /// A box's border box, or one piece of a split inline box
    Box,
    /// A line box of an inline formatting context
    Line,
    /// A run of one text node's text on one line
    Text,
}

#[derive(Debug)]
pub struct Fragment {
    pub kind: FragKind,
    /// The element or text node it is of; `None` for line boxes and anonymous boxes
    pub node: Option<NodeId>,
    pub size: Size,
    pub children: Box<[Child]>,
    /// It is a scroll container: what overflows it is scrolled to, and reaches nothing above it
    pub scroller: bool,
    /// From its corner, its border box and what of its descendants overflows it, a scroll container's
    /// stopping at their border boxes: for a scroll container, its scrollable overflow area
    pub overflow: Rect,
}

#[derive(Clone, Debug)]
pub struct Child {
    pub offset: Point,
    pub frag: Arc<Fragment>,
}

impl Fragment {
    pub fn new(
        kind: FragKind,
        node: Option<NodeId>,
        size: Size,
        children: Vec<Child>,
    ) -> Arc<Fragment> {
        Fragment::with_scroller(kind, node, size, children, false)
    }

    /// The same for a box that is a scroll container (`scroller`) or not.
    pub fn with_scroller(
        kind: FragKind,
        node: Option<NodeId>,
        size: Size,
        children: Vec<Child>,
        scroller: bool,
    ) -> Arc<Fragment> {
        let mut overflow = Rect {
            x: Au::ZERO,
            y: Au::ZERO,
            w: size.w,
            h: size.h,
        };
        for c in &children {
            let r = c.frag.reach();
            overflow = overflow.union(Rect {
                x: r.x + c.offset.x,
                y: r.y + c.offset.y,
                ..r
            });
        }
        Arc::new(Fragment {
            kind,
            node,
            size,
            children: children.into_boxed_slice(),
            scroller,
            overflow,
        })
    }

    /// What of it its parent's overflow takes: its border box if it scrolls, else its overflow.
    fn reach(&self) -> Rect {
        if self.scroller {
            Rect {
                x: Au::ZERO,
                y: Au::ZERO,
                w: self.size.w,
                h: self.size.h,
            }
        } else {
            self.overflow
        }
    }

    /// Every fragment below and including this one with its offset from this one's corner, in tree order;
    /// an explicit stack, so a deep tree cannot overflow the call stack.
    pub fn walk(self: &Arc<Fragment>, mut f: impl FnMut(Point, &Arc<Fragment>)) {
        let mut stack = vec![(Point::default(), self.clone())];
        while let Some((at, frag)) = stack.pop() {
            f(at, &frag);
            for c in frag.children.iter().rev() {
                stack.push((at + c.offset, c.frag.clone()));
            }
        }
    }
}
