//! The fragment tree: what layout produces. A fragment is immutable and shared; it knows its own size and
//! where each child sits relative to its own top left corner, never its own position, so an unchanged child
//! can hang under a new parent as it is.

use std::sync::Arc;

use madcawl_dom::NodeId;

use crate::au::{Point, Size};

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
        Arc::new(Fragment {
            kind,
            node,
            size,
            children: children.into_boxed_slice(),
        })
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
