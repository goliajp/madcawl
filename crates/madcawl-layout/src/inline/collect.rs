//! An inline formatting context flattened: one string of its text with white space processed, an item per
//! piece of it (a text node's run, an atomic inline as U+FFFC, a `<br>` as a newline), the inline boxes with
//! the text range each covers, then shaped into segments with their advances.

use std::ops::Range;

use madcawl_css::style::{TextWrapMode, WhiteSpaceCollapse};
use madcawl_text::{After, Direction, Kind as SegKind, Span, Style};

use madcawl_dom::NodeId;

use crate::au::Au;
use crate::cx::Cx;
use crate::style::Edges;
use crate::tree::{Kind, NONE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Atomic,
    Break,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: ItemKind,
    /// The text, atomic or `<br>` box
    pub b: u32,
    pub node: NodeId,
    pub range: Range<usize>,
    /// The innermost inline box around it, `NONE` for the formatting context's root
    pub parent: u32,
    pub wrap: bool,
    /// Its white space is kept: a line with only that is not empty
    pub preserve: bool,
}

/// An inline box of the context and the text it covers.
#[derive(Clone, Debug)]
pub struct InlineBox {
    pub b: u32,
    pub node: NodeId,
    pub parent: u32,
    pub range: Range<usize>,
    /// The segments it starts and ends in, when it covers any
    pub segs: Option<(usize, usize)>,
    /// Inline-start margin and border plus padding, and inline-end ones
    pub start: (Au, Au),
    pub end: (Au, Au),
}

/// A shaped piece of the text with no break opportunity inside.
#[derive(Clone, Debug)]
pub struct Seg {
    pub range: Range<usize>,
    pub item: usize,
    pub kind: SegKind,
    pub after: After,
    /// The advance, and for an atomic inline its margin box width
    pub adv: Au,
    /// Inline-start and inline-end margins, borders and padding of the inline boxes starting and ending here
    pub lead: Au,
    pub trail: Au,
}

#[derive(Default)]
pub struct Ifc {
    pub text: Vec<u16>,
    pub items: Vec<Item>,
    pub boxes: Vec<InlineBox>,
    pub segs: Vec<Seg>,
    /// The floats, each with the text offset it comes at
    pub floats: Vec<(usize, u32)>,
    /// The out-of-flow boxes, each with the text offset it comes at
    pub abs: Vec<(usize, u32)>,
    spans: Vec<Span>,
}

fn is_space(u: u16) -> bool {
    matches!(u, 0x20 | 0x09 | 0x0a | 0x0c | 0x0d)
}

impl Ifc {
    // appends a text node's data as its style processes white space
    fn push_text(&mut self, data: impl Iterator<Item = u16>, collapse: WhiteSpaceCollapse) {
        for u in data {
            let prev_space = self.text.last().is_none_or(|&l| l == 0x20 || l == 0x0a);
            match collapse {
                WhiteSpaceCollapse::Collapse if is_space(u) => {
                    if !prev_space {
                        self.text.push(0x20);
                    }
                }
                WhiteSpaceCollapse::PreserveBreaks if u == 0x0a => {
                    if self.text.last() == Some(&0x20) {
                        self.text.pop();
                    }
                    self.text.push(0x0a);
                }
                WhiteSpaceCollapse::PreserveBreaks if is_space(u) => {
                    if !prev_space {
                        self.text.push(0x20);
                    }
                }
                _ if u == 0x0d => {}
                _ => self.text.push(u),
            }
        }
    }

    fn item(
        &mut self,
        kind: ItemKind,
        b: (u32, NodeId),
        start: usize,
        parent: u32,
        ws: (bool, bool),
    ) {
        if self.text.len() > start {
            self.items.push(Item {
                kind,
                b: b.0,
                node: b.1,
                range: start..self.text.len(),
                parent,
                wrap: ws.0,
                preserve: ws.1,
            });
        }
    }
}

/// Flattens the inline-level children of block container `root`.
pub fn collect(cx: &mut Cx, root: u32, out: &mut Ifc) {
    out.text.clear();
    out.items.clear();
    out.boxes.clear();
    out.floats.clear();
    out.abs.clear();
    // (box, the inline box it is in, the index of its entry in `boxes` when it is one being closed)
    let mut stack: Vec<(u32, u32, usize)> = cx.tree.children(root).map(|c| (c, NONE, 0)).collect();
    stack.reverse();
    while let Some((b, parent, close)) = stack.pop() {
        if close != 0 {
            out.boxes[close - 1].range.end = out.text.len();
            continue;
        }
        let s = cx.style(b);
        let wrap = (
            s.text.text_wrap_mode == TextWrapMode::Wrap,
            s.text.white_space_collapse != WhiteSpaceCollapse::Collapse,
        );
        let start = out.text.len();
        let node = cx.tree.get(b).node;
        let bn = (b, node);
        match cx.tree.get(b).kind {
            Kind::Text => {
                if let Some(t) = cx.dom.text(cx.tree.get(b).node) {
                    out.push_text(t.units(), s.text.white_space_collapse);
                }
                out.item(ItemKind::Text, bn, start, parent, wrap);
            }
            Kind::Break => {
                out.text.push(0x0a);
                out.item(ItemKind::Break, bn, start, parent, wrap);
            }
            Kind::Inline => {
                let e = Edges::of(s, Au::ZERO);
                out.boxes.push(InlineBox {
                    b,
                    node,
                    parent,
                    range: start..start,
                    segs: None,
                    start: (e.m(3), e.bp(3)),
                    end: (e.m(1), e.bp(1)),
                });
                stack.push((b, parent, out.boxes.len()));
                let at = stack.len();
                stack.extend(cx.tree.children(b).map(|c| (c, b, 0)));
                stack[at..].reverse();
            }
            // out of flow: laid out by its containing block
            Kind::Block if crate::position::out_of_flow(s) => out.abs.push((start, b)),
            Kind::Block if s.float != madcawl_css::style::Float::None => {
                out.floats.push((start, b))
            }
            _ => {
                out.text.push(0xfffc);
                out.item(ItemKind::Atomic, bn, start, parent, wrap);
            }
        }
    }
}

/// Shapes the collected text; `atomic` gives each atomic item's margin box width, by item.
pub fn shape(cx: &mut Cx, root: u32, ifc: &mut Ifc, atomic: &[Au]) {
    ifc.spans.clear();
    for it in &ifc.items {
        let f = cx.font(it.b);
        ifc.spans.push(Span {
            range: it.range.clone(),
            style: Style {
                face: f.face,
                size: f.size,
                features: 0,
            },
        });
    }
    let dir = match cx.style(root).text.direction {
        madcawl_css::style::Direction::Rtl => Direction::Rtl,
        madcawl_css::style::Direction::Ltr => Direction::Ltr,
    };
    cx.para.shape(cx.shaper, &ifc.text, &ifc.spans, dir);
    ifc.segs.clear();
    for s in cx.para.segments() {
        let item = &ifc.items[s.span];
        let adv = match item.kind {
            ItemKind::Atomic => atomic[s.span],
            ItemKind::Break => Au::ZERO,
            ItemKind::Text => Au::from_px(s.advance()),
        };
        // a break opportunity in text that does not wrap is none
        let after = match s.after {
            After::Break if !item.wrap || !ascii_break(&ifc.text, s.range.end) => After::Join,
            a => a,
        };
        ifc.segs.push(Seg {
            range: s.range.clone(),
            item: s.span,
            kind: s.kind,
            after,
            adv,
            lead: Au::ZERO,
            trail: Au::ZERO,
        });
    }
    for k in 0..ifc.boxes.len() {
        let r = ifc.boxes[k].range.clone();
        if r.is_empty() {
            // an empty box's borders and padding come before what follows it
            let at = ifc.segs.partition_point(|s| s.range.start < r.start);
            let ib = &ifc.boxes[k];
            if let Some(s) = ifc.segs.get_mut(at) {
                s.lead += ib.start.0 + ib.start.1 + ib.end.0 + ib.end.1;
            }
            continue;
        }
        let first = ifc.segs.partition_point(|s| s.range.end <= r.start);
        let last = ifc.segs.partition_point(|s| s.range.start < r.end);
        if first >= last {
            continue;
        }
        let ib = &mut ifc.boxes[k];
        ib.segs = Some((first, last - 1));
        ifc.segs[first].lead += ib.start.0 + ib.start.1;
        ifc.segs[last - 1].trail += ib.end.0 + ib.end.1;
    }
}

// between two printable ASCII characters WebKit breaks only after `-` and `?` before a letter or digit, and
// before an opening bracket after one; a break UAX #14 finds anywhere else between them (after `/`, `%`,
// before `$` and the like) is not taken
fn ascii_break(text: &[u16], at: usize) -> bool {
    let (Some(&a), Some(&b)) = (text.get(at.wrapping_sub(1)), text.get(at)) else {
        return true;
    };
    let graphic = |u: u16| (0x21..=0x7e).contains(&u);
    if !graphic(a) || !graphic(b) {
        return true;
    }
    let (a, b) = (a as u8, b as u8);
    matches!(a, b'-' | b'?') && b.is_ascii_alphanumeric()
        || matches!(b, b'(' | b'[' | b'{' | b'<') && a.is_ascii_alphanumeric()
}
