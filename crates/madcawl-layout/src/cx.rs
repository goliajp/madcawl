//! What every layout function takes: the document, the box tree, fonts, and the cache of the last result
//! per box.

use std::sync::Arc;

use madcawl_css::style::{ComputedStyle, Styles};
use madcawl_dom::{Dom, NodeKind};
use madcawl_text::{Paragraph, Shaper};

use crate::au::{Au, Point, Size};
use crate::float::Exclusions;
use crate::fragment::Fragment;
use crate::style::{Edges, FontInfo, FontTable};
use crate::tree::{BoxTree, Kind};

/// The constraint space a box is laid out in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    /// The containing block's width
    pub width: Au,
    /// The containing block's height, when definite: what percentage heights are of
    pub height: Option<Au>,
    /// The content-box width and height the box must take whatever its own say, as a flex container
    /// gives its items
    pub fixed: [Option<Au>; 2],
}

impl Space {
    pub fn new(width: Au, height: Option<Au>) -> Space {
        Space {
            width,
            height,
            fixed: [None, None],
        }
    }
}

/// Adjoining margins, collapsed: the largest positive one and the most negative one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Strut {
    pub pos: Au,
    pub neg: Au,
}

impl Strut {
    pub fn of(m: Au) -> Strut {
        let mut s = Strut::default();
        s.add_margin(m);
        s
    }

    pub fn add_margin(&mut self, m: Au) {
        self.pos = self.pos.max(m);
        self.neg = self.neg.min(m);
    }

    pub fn add(&mut self, o: Strut) {
        self.pos = self.pos.max(o.pos);
        self.neg = self.neg.min(o.neg);
    }

    pub fn resolve(self) -> Au {
        self.pos + self.neg
    }
}

/// A block-level box laid out: its fragment (the border box) and what its parent places it by.
#[derive(Clone, Debug)]
pub struct Laid {
    pub frag: Arc<Fragment>,
    /// Its top margin, collapsed with those of its first children that adjoin it
    pub mt: Strut,
    pub mb: Strut,
    /// The used left margin, which decides where it starts in its containing block
    pub ml: Au,
    pub mr: Au,
    /// Its top and bottom margins adjoin: it has no content, no height, borders or padding
    pub through: bool,
    /// The baseline of its last line, from its top border edge
    pub baseline: Option<Au>,
    /// Its height was auto and its minimum or maximum changed it: laid out at that height as a fixed one, its
    /// contents would have had a definite height to fill
    pub clamped: bool,
    /// Floats outside it had a say in it, or it added floats to the formatting context around it: the result
    /// holds only where it was
    pub floats: bool,
    /// The absolutely and fixed positioned boxes below it whose containing block is above it, each with its
    /// static position from this box's border box corner
    pub statics: Vec<(u32, Point)>,
}

pub struct Cx<'a> {
    pub dom: &'a Dom,
    pub styles: &'a Styles,
    pub tree: &'a BoxTree,
    pub shaper: &'a mut Shaper,
    pub fonts: &'a mut FontTable,
    pub para: &'a mut Paragraph,
    pub cache: &'a mut [crate::cache::Slots],
    /// By box, its last results laid out while only its size was wanted and whose fragments are not final
    pub measures: &'a mut [crate::cache::Slots],
    /// By box, whether a different containing block height can change it
    pub height_readers: &'a [bool],
    /// By box, its min-content and max-content widths
    pub intrinsic: &'a mut [Option<(Au, Au)>],
    /// The initial containing block: the viewport
    pub icb: Size,
    /// The floats of each block formatting context being laid out, innermost last
    pub excl: &'a mut Vec<Exclusions>,
    pub counters: &'a mut Counters,
    /// Whether a box may be handed its last result instead of being laid out
    pub reuse: bool,
    /// Device pixels per CSS px
    pub scale: f32,
    /// Only the size of what is laid out is wanted: a flex container leaves out stretching its items, which
    /// does not change its own size
    pub measuring: bool,
    /// Grows each time measuring leaves a result inexact: a box laid out while it grew holds only its size
    pub inexact: u64,
}

/// What a layout did: boxes laid out, how many of them reused the last result, and the float and inline box
/// entries looked at while placing floats and building lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub laid: u64,
    pub reused: u64,
    pub float_visits: u64,
    pub inline_box_visits: u64,
}

impl<'a> Cx<'a> {
    /// The computed values a box takes: its element's, or for text and anonymous boxes the parent's; a
    /// pseudo-element's own for its box and text.
    pub fn style(&self, b: u32) -> &'a ComputedStyle {
        let bx = self.tree.get(b);
        if bx.pseudo != 0 {
            return self
                .styles
                .pseudo(bx.node, bx.pseudo == 2)
                .unwrap_or_else(|| ComputedStyle::initial());
        }
        let n = bx.node;
        let el = if self.dom.kind(n) == NodeKind::Element {
            Some(n)
        } else {
            self.dom.parent(n)
        };
        el.and_then(|e| self.styles.get(e))
            .unwrap_or_else(|| ComputedStyle::initial())
    }

    /// Runs `f` measuring or not; a result laid out not measuring is exact whatever was measured inside it.
    pub fn measure<T>(&mut self, on: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        let (was, inexact) = (self.measuring, self.inexact);
        self.measuring = on;
        let r = f(self);
        self.measuring = was;
        if !on {
            self.inexact = inexact;
        }
        r
    }

    /// Whether the box has a box model of its own: anonymous and text boxes have none.
    pub fn has_edges(&self, b: u32) -> bool {
        !matches!(
            self.tree.get(b).kind,
            Kind::AnonBlock | Kind::Text | Kind::Generated(_)
        )
    }

    /// Whether the box's `overflow` is the viewport's instead of its own: the root's, and the body's when
    /// the root's is `visible`.
    pub fn overflow_propagated(&self, b: u32) -> bool {
        let bx = self.tree.get(b);
        if bx.parent == crate::tree::NONE {
            return true;
        }
        let root = self.tree.get(bx.parent);
        root.parent == crate::tree::NONE
            && bx.pseudo == 0
            && self.dom.namespace(bx.node) == Some(madcawl_html::Namespace::Html)
            && self.dom.local_name(bx.node) == Some(madcawl_str::atom!("body"))
            && {
                let rs = self.style(bx.parent);
                rs.place.overflow_x == madcawl_css::style::Overflow::Visible
                    && rs.place.overflow_y == madcawl_css::style::Overflow::Visible
            }
    }

    /// Whether the box clips or scrolls what overflows it.
    pub fn scroller(&self, b: u32) -> bool {
        use madcawl_css::style::Overflow;
        let s = self.style(b);
        self.has_edges(b)
            && (s.place.overflow_x != Overflow::Visible || s.place.overflow_y != Overflow::Visible)
            && !self.overflow_propagated(b)
    }

    /// The node a box's fragments are of: none for a pseudo-element's, whose boxes are not its element's.
    pub fn frag_node(&self, b: u32) -> Option<madcawl_dom::NodeId> {
        let bx = self.tree.get(b);
        (bx.pseudo == 0).then_some(bx.node)
    }

    /// A box's margins, borders and padding against `cw`, its border widths whole device pixels: a border
    /// narrower than one device pixel takes one, as WebKit draws them.
    pub fn edges(&self, s: &ComputedStyle, cw: Au) -> Edges {
        let mut e = Edges::of(s, cw);
        for b in &mut e.border {
            *b = device_floor(*b, self.scale);
        }
        e
    }

    pub fn font(&mut self, b: u32) -> FontInfo {
        let font = self.style(b).font.clone();
        self.fonts.get(&mut self.shaper.fonts, &font)
    }
}

/// A non-zero length floored to whole device pixels at `scale`, and at least one.
pub fn device_floor(v: Au, scale: f32) -> Au {
    if v <= Au::ZERO || scale <= 0.0 {
        return v;
    }
    let dev = (v.px() * scale).floor().max(1.0);
    Au::from_px(dev / scale)
}
