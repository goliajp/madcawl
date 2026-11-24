//! Layout: the box tree from the DOM and its computed values (`tree`), and the fragment tree layout makes of
//! it (`fragment`). Each layout algorithm is a function from a box and its constraint space to a fragment;
//! a box keeps its last (space, result) and hands it back while it is not dirty and the space is the same.
//!
//! Block layout (`block`) does widths and heights with their limits and `box-sizing`, percentages, and
//! margin collapsing; inline layout (`inline`) flattens an inline formatting context, shapes it through
//! `madcawl-text`, breaks it into lines and builds line boxes from the inline boxes and atomic inlines on
//! them. Out-of-flow boxes are placed against their containing block (`position`).
//!
//! Nothing is laid out until something asks for geometry: a [`Layout`] holds the document and does no work
//! until [`Layout::fragments`] or [`Layout::border_box`] is called.
//!
//! Lengths are 1/64 px fixed point and saturate. The box tree's depth is bounded (`tree::MAX_DEPTH`), which
//! bounds the recursion of layout over it.

mod au;
mod block;
mod cache;
mod cx;
mod flex;
mod float;
mod fragment;
mod inline;
mod intrinsic;
mod position;
mod replaced;
mod sizing;
mod style;
pub mod tree;

use std::collections::HashMap;

use madcawl_css::style::Styles;
use madcawl_dom::{Dom, NodeId};
use madcawl_text::{Paragraph, Shaper};

pub use au::{Au, Point, Rect, Size};
pub use cx::{Counters, Space};
pub use fragment::{Child, FragKind, Fragment};
pub use tree::{BoxTree, MAX_DEPTH};

use cx::Cx;
use style::FontTable;

struct State {
    tree: BoxTree,
    cache: Vec<cache::Slots>,
    height_readers: Vec<bool>,
    intrinsic: Vec<Option<(Au, Au)>>,
    /// The root element's fragment and where it is on the canvas
    root: Option<Child>,
    rects: Option<HashMap<NodeId, Rect>>,
}

/// The layout of one document, made when first asked for.
pub struct Layout<'d> {
    dom: &'d Dom,
    styles: &'d Styles,
    viewport: Size,
    shaper: Shaper,
    fonts: FontTable,
    para: Paragraph,
    state: Option<State>,
    counters: Counters,
    runs: u64,
    reuse: bool,
}

impl<'d> Layout<'d> {
    /// The layout of `dom` under `styles` in a viewport of `width` by `height` CSS px; nothing is done yet.
    pub fn new(dom: &'d Dom, styles: &'d Styles, width: f32, height: f32) -> Self {
        Layout {
            dom,
            styles,
            viewport: Size {
                w: Au::from_px(width),
                h: Au::from_px(height),
            },
            shaper: Shaper::default(),
            fonts: FontTable::default(),
            para: Paragraph::new(),
            state: None,
            counters: Counters::default(),
            runs: 0,
            reuse: true,
        }
    }

    /// How many times the document has been laid out.
    pub fn runs(&self) -> u64 {
        self.runs
    }

    /// Lays every box out every time it is asked for instead of reusing its last result, to check that reuse
    /// changes nothing.
    #[doc(hidden)]
    pub fn set_reuse(&mut self, on: bool) {
        self.reuse = on;
        self.state = None;
    }

    /// Whether the box tree has been built.
    pub fn has_boxes(&self) -> bool {
        self.state.is_some()
    }

    /// Boxes laid out and results reused, over every run.
    pub fn counters(&self) -> Counters {
        self.counters
    }

    pub fn boxes(&mut self) -> &BoxTree {
        &self.state().tree
    }

    fn state(&mut self) -> &mut State {
        self.state.get_or_insert_with(|| {
            let tree = tree::build_tree(self.dom, self.styles);
            let n = tree.len();
            let height_readers = cache::height_readers(self.dom, self.styles, &tree);
            State {
                tree,
                cache: vec![cache::EMPTY; n],
                height_readers,
                intrinsic: vec![None; n],
                root: None,
                rects: None,
            }
        })
    }

    /// The root element's fragment and its offset on the canvas, laying the document out if it is not.
    pub fn fragments(&mut self) -> Option<&Child> {
        if self.state.as_ref().is_none_or(|s| s.root.is_none()) {
            self.run();
        }
        self.state.as_ref().and_then(|s| s.root.as_ref())
    }

    fn run(&mut self) {
        self.state();
        let st = self.state.as_mut().expect("built above");
        let Some(root) = st.tree.root() else { return };
        self.runs += 1;
        let mut cx = Cx {
            dom: self.dom,
            styles: self.styles,
            tree: &st.tree,
            shaper: &mut self.shaper,
            fonts: &mut self.fonts,
            para: &mut self.para,
            cache: &mut st.cache,
            height_readers: &st.height_readers,
            intrinsic: &mut st.intrinsic,
            icb: self.viewport,
            excl: &mut Vec::new(),
            counters: &mut self.counters,
            reuse: self.reuse,
        };
        let space = Space::new(self.viewport.w, Some(self.viewport.h));
        let r = block::layout(&mut cx, root, space);
        st.root = Some(Child {
            offset: Point {
                x: r.ml,
                y: r.mt.resolve(),
            },
            frag: r.frag,
        });
        st.rects = None;
    }

    /// The union of the border boxes of `n`'s fragments on the canvas, as `getBoundingClientRect` gives it
    /// with nothing scrolled; `None` when it has no box.
    pub fn border_box(&mut self, n: NodeId) -> Option<Rect> {
        self.fragments()?;
        let st = self.state.as_mut()?;
        let root = st.root.as_ref()?;
        let rects = st.rects.get_or_insert_with(|| {
            let mut m: HashMap<NodeId, Rect> = HashMap::new();
            root.frag.walk(|at, f| {
                let Some(node) = f.node else { return };
                if f.kind == FragKind::Text {
                    return;
                }
                let at = at + root.offset;
                let r = Rect {
                    x: at.x,
                    y: at.y,
                    w: f.size.w,
                    h: f.size.h,
                };
                m.entry(node).and_modify(|o| *o = o.union(r)).or_insert(r);
            });
            for &(b, n) in &st.tree.hoisted {
                if let Some(&r) = m.get(&st.tree.get(b).node) {
                    m.entry(n).and_modify(|o| *o = o.union(r)).or_insert(r);
                }
            }
            m
        });
        rects.get(&n).copied()
    }

    /// Marks `n`'s computed values as changed: its boxes and their ancestors are laid out again on the next
    /// request, every other box reusing its last result.
    pub fn invalidate(&mut self, n: NodeId) {
        let Some(st) = self.state.as_mut() else {
            return;
        };
        for b in 0..st.tree.len() as u32 {
            if st.tree.get(b).node != n {
                continue;
            }
            cache::mark(self.dom, self.styles, &st.tree, &mut st.height_readers, b);
            let mut up = b;
            while up != tree::NONE {
                st.cache[up as usize] = cache::EMPTY;
                st.intrinsic[up as usize] = None;
                up = st.tree.get(up).parent;
            }
        }
        st.root = None;
        st.rects = None;
    }

    /// Drops the box tree, for a change to the document's structure.
    pub fn rebuild(&mut self) {
        self.state = None;
    }
}
