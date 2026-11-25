//! Line content moved from its place on the baseline: relatively positioned inline boxes, and inline boxes
//! aligned with the line's top or bottom, each taking what is in it along; and the out-of-flow boxes whose
//! containing block is a positioned inline box of the context.

use madcawl_css::style::Position;

use madcawl_css::style::{VAlign, VerticalAlign};

use super::Lines;
use super::line::{Builder, VBox};
use crate::au::{Au, Point, Rect};
use crate::cx::Cx;
use crate::fragment::FragKind;
use crate::style::{FontInfo, half_leading, lp};
use crate::tree::{Kind, NONE};

// the baseline shift `vertical-align` gives a box in a parent of font size `parent`, `lh` being its own
// line height
pub(super) fn valign_shift(v: VAlign, parent: f32, lh: Au) -> Au {
    match v {
        VAlign::Lp(l) => -lp(l, lh),
        VAlign::Kw(VerticalAlign::Sub) => Au::from_px((parent / 5.0 + 1.0).floor()),
        VAlign::Kw(VerticalAlign::Super) => -Au::from_px((parent / 3.0 + 1.0).floor()),
        VAlign::Kw(_) => Au::ZERO,
    }
}

pub(super) fn vbox(f: &FontInfo, lh: Au) -> VBox {
    let (above, below) = half_leading(f, lh);
    VBox {
        above,
        below,
        asc: f.ascent,
        desc: f.descent,
        size: f.size,
        x_height: f.x_height,
        ..VBox::default()
    }
}

impl Builder<'_> {
    /// Where inline box `k`'s pieces on the current line move.
    pub(super) fn box_off(&self, k: usize) -> Point {
        let v = &self.boxes[k];
        let dy = v.edge.map_or(Au::ZERO, |(a, _)| self.edge_dy[a]);
        Point {
            x: v.rel.x,
            y: v.rel.y + dy,
        }
    }

    /// Where an item moves: with the inline box it is in.
    pub(super) fn item_off(&self, item: usize) -> Point {
        let p = self.ifc.items[item].parent;
        self.index
            .get(&p)
            .map_or(Point::default(), |&k| self.box_off(k))
    }

    /// Whether an item is in a box aligned with the line's top or bottom, which places it.
    pub(super) fn item_aligned(&self, item: usize) -> bool {
        let p = self.ifc.items[item].parent;
        self.index
            .get(&p)
            .is_some_and(|&k| self.boxes[k].edge.is_some())
    }

    /// The boxes of `on` aligned with the line's top or bottom, once the rest gave the line `top` and
    /// `bottom` from the root's baseline: each stretches the line when taller, then goes to its edge.
    pub(super) fn align_edges(&mut self, on: &[usize], top: &mut Au, bottom: &mut Au) {
        for &k in on {
            let v = self.boxes[k];
            match v.edge {
                Some((a, true)) if a == k => *bottom = (*bottom).max(*top + v.above + v.below),
                Some((a, false)) if a == k => *top = (*top).min(*bottom - v.above - v.below),
                _ => {}
            }
        }
        for &k in on {
            let v = self.boxes[k];
            match v.edge {
                Some((a, true)) if a == k => self.edge_dy[k] = *top + v.above - v.shift,
                Some((a, false)) if a == k => self.edge_dy[k] = *bottom - v.below - v.shift,
                _ => {}
            }
        }
    }

    pub(super) fn reset_edges(&mut self, on: &[usize]) {
        for &k in on {
            self.edge_dy[k] = Au::ZERO;
        }
    }
}

// the nearest positioned inline box around `a` below the context's root `root`
fn positioned_inline(cx: &Cx, root: u32, a: u32) -> Option<u32> {
    let mut up = cx.tree.get(a).parent;
    while up != NONE && up != root {
        if cx.tree.get(up).kind == Kind::Inline && cx.style(up).position != Position::Static {
            return Some(up);
        }
        up = cx.tree.get(up).parent;
    }
    None
}

// the bounding box of the padding boxes of `ib`'s pieces on the lines
fn pieces_box(cx: &Cx, ib: u32, lines: &Lines, cw: Au) -> Option<Rect> {
    let node = cx.frag_node(ib)?;
    let mut r: Option<Rect> = None;
    for line in &lines.lines {
        for c in line.frag.children.iter() {
            if c.frag.kind != FragKind::Box || c.frag.node != Some(node) {
                continue;
            }
            let at = line.offset + c.offset;
            let p = Rect {
                x: at.x,
                y: at.y,
                w: c.frag.size.w,
                h: c.frag.size.h,
            };
            r = Some(r.map_or(p, |o| o.union(p)));
        }
    }
    let b = cx.edges(cx.style(ib), cw).border;
    r.map(|r| Rect {
        x: r.x + b[3],
        y: r.y + b[0],
        w: (r.w - b[1] - b[3]).max(Au::ZERO),
        h: (r.h - b[0] - b[2]).max(Au::ZERO),
    })
}

/// Lays out the out-of-flow boxes among `out`'s statics whose containing block is a positioned inline box
/// of the context rooted at `root`, against it, and adds them to the lines; the rest stay for the block.
pub fn place_in_inlines(cx: &mut Cx, root: u32, out: &mut Lines, cw: Au) {
    if out.statics.is_empty() {
        return;
    }
    let statics = std::mem::take(&mut out.statics);
    let mut keep = Vec::new();
    for (a, p) in statics {
        let cb = (cx.style(a).position != Position::Fixed)
            .then(|| positioned_inline(cx, root, a))
            .flatten()
            .and_then(|ib| pieces_box(cx, ib, out, cw));
        match cb {
            Some(r) => {
                let up = crate::position::place_in(cx, r, false, &mut out.lines, vec![(a, p)]);
                keep.extend(up);
            }
            None => keep.push((a, p)),
        }
    }
    out.statics = keep;
}
