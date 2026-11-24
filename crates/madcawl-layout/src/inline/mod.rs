//! Inline layout: an inline formatting context flattened and shaped (`collect`), broken into lines greedily
//! at its break opportunities, and each line box given its height from the inline boxes and atomic inlines
//! on it (`line`).

mod collect;
mod line;
mod pieces;

use std::ops::Range;

use madcawl_text::{After, Kind as SegKind};

use madcawl_css::style::Float;

use crate::au::{Au, Point};
use crate::block;
use crate::cx::{Cx, Laid, Space};
use crate::fragment::Child;
use crate::style::Edges;

pub use collect::{Ifc, ItemKind};

/// The lines of an inline formatting context.
pub struct Lines {
    pub lines: Vec<Child>,
    pub height: Au,
    /// The last line's baseline, from the top of the first
    pub baseline: Option<Au>,
    /// Every line is empty: the context takes no room
    pub empty: bool,
    /// Out-of-flow boxes among the lines, from the content box corner
    pub statics: Vec<(u32, Point)>,
}

/// A line: its segments and the width of what does not hang at its end.
pub struct LineRange {
    pub segs: Range<usize>,
    pub width: Au,
}

fn seg_width(s: &collect::Seg) -> Au {
    s.lead + s.adv + s.trail
}

/// One line from segment `start` at `avail`: words while they fit, white space at its end hanging, a word
/// wider than the line alone on it, up to a mandatory break.
pub fn next_line(segs: &[collect::Seg], start: usize, avail: Au) -> LineRange {
    let (mut w, mut hang) = (Au::ZERO, Au::ZERO);
    let mut k = start;
    while k < segs.len() {
        let word = k;
        let (mut ww, mut wh, mut text) = (Au::ZERO, Au::ZERO, false);
        while k < segs.len() {
            let a = seg_width(&segs[k]);
            if segs[k].kind == SegKind::Space {
                wh += a;
            } else {
                (wh, text) = (Au::ZERO, true);
            }
            ww += a;
            k += 1;
            if segs[k - 1].after != After::Join {
                break;
            }
        }
        if word > start && text && w + ww - wh > avail {
            return LineRange {
                segs: start..word,
                width: w - hang,
            };
        }
        w += ww;
        hang = if text { wh } else { hang + wh };
        if segs[k - 1].after == After::Mandatory {
            break;
        }
    }
    LineRange {
        segs: start..k,
        width: w - hang,
    }
}

// the atomic inlines laid out at the context's width, by item, and their margin box widths
fn atomics(cx: &mut Cx, ifc: &Ifc, inner: Space) -> (Vec<Option<Laid>>, Vec<Au>) {
    let mut laid = Vec::with_capacity(ifc.items.len());
    let mut widths = Vec::with_capacity(ifc.items.len());
    for it in &ifc.items {
        if it.kind == ItemKind::Atomic {
            let r = block::layout(cx, it.b, inner);
            widths.push(r.frag.size.w + r.ml + r.mr);
            laid.push(Some(r));
        } else {
            widths.push(Au::ZERO);
            laid.push(None);
        }
    }
    (laid, widths)
}

// lays out a float and places it in the formatting context at `y` or below; its fragment, from the
// content box of the block whose lines it is among
fn float(
    cx: &mut Cx,
    f: u32,
    (inner, at, y): (Space, Point, Au),
    statics: &mut Vec<(u32, Point)>,
) -> Child {
    let r = block::layout(cx, f, inner);
    let s = cx.style(f);
    let e = Edges::of(s, inner.width);
    let w = r.ml + r.frag.size.w + r.mr;
    let h = e.m(0) + r.frag.size.h + e.m(2);
    let left = s.float != Float::Right;
    let ex = cx.excl.last_mut().expect("a formatting context");
    let y = ex.clear(at.y + y, s.place.clear);
    let p = ex.place(left, w, h, y, at.x, at.x + inner.width);
    let offset = Point {
        x: p.x - at.x + r.ml,
        y: p.y - at.y + e.m(0),
    };
    statics.extend(r.statics.iter().map(|&(a, q)| (a, q + offset)));
    Child {
        offset,
        frag: r.frag,
    }
}

/// Lays out the inline-level children of block container `b`, whose content box is `inner` and sits at `at`
/// in its formatting context.
pub fn layout(cx: &mut Cx, b: u32, inner: Space, at: Point) -> Lines {
    let mut ifc = Ifc::default();
    collect::collect(cx, b, &mut ifc);
    let (laid, widths) = atomics(cx, &ifc, inner);
    collect::shape(cx, b, &mut ifc, &widths);
    let indent = crate::style::lp(cx.style(b).text.text_indent, inner.width);
    let mut lb = line::Builder::new(cx, b, &ifc, &laid, inner);
    let (mut floats, mut fstatics) = (Vec::new(), Vec::new());
    let (mut k, mut fl, mut first) = (0, 0, true);
    while k < ifc.segs.len() {
        while fl < ifc.floats.len() && ifc.floats[fl].0 <= ifc.segs[k].range.start {
            floats.push(float(
                cx,
                ifc.floats[fl].1,
                (inner, at, lb.y()),
                &mut fstatics,
            ));
            fl += 1;
        }
        let ind = if first { indent } else { Au::ZERO };
        let (left, avail) = room(cx, at, inner, lb.y(), lb.strut_height());
        let r = next_line(&ifc.segs, k, avail - ind);
        // a float among the line's content goes at the line's top when it fits, and the line is broken
        // again in the room left
        let end = ifc.segs[r.segs.end - 1].range.end;
        if fl < ifc.floats.len() && ifc.floats[fl].0 < end {
            while fl < ifc.floats.len() && ifc.floats[fl].0 < end {
                floats.push(float(
                    cx,
                    ifc.floats[fl].1,
                    (inner, at, lb.y()),
                    &mut fstatics,
                ));
                fl += 1;
            }
            continue;
        }
        // a line that does not fit beside floats goes below them
        if r.width > avail - ind
            && let Some(ex) = cx.excl.last()
            && let Some(below) = ex.next_bottom(at.y + lb.y())
            && avail < inner.width
        {
            lb.move_to(below - at.y);
            continue;
        }
        k = r.segs.end;
        lb.line(cx, &r, left + ind, avail - ind);
        first = false;
    }
    for &(_, f) in &ifc.floats[fl..] {
        floats.push(float(cx, f, (inner, at, lb.y()), &mut fstatics));
    }
    let mut out = lb.finish();
    out.lines.extend(floats);
    out.statics.extend(fstatics);
    out
}

// the left edge and width a line at `y` gets beside the floats
fn room(cx: &Cx, at: Point, inner: Space, y: Au, h: Au) -> (Au, Au) {
    match cx.excl.last().filter(|e| !e.is_empty()) {
        None => (Au::ZERO, inner.width),
        Some(ex) => {
            let (l, r) = ex.band(at.y + y, h, at.x, at.x + inner.width);
            (l - at.x, (r - l).max(Au::ZERO))
        }
    }
}

/// The narrowest and widest the context can be: its widest word, and its widest line with no breaks but
/// the forced ones.
pub fn content_widths(cx: &mut Cx, b: u32, cw: Au) -> (Au, Au) {
    let mut ifc = Ifc::default();
    collect::collect(cx, b, &mut ifc);
    let mut widths = Vec::with_capacity(ifc.items.len());
    for it in &ifc.items {
        widths.push(if it.kind == ItemKind::Atomic {
            crate::intrinsic::margin_box_widths(cx, it.b, cw).1
        } else {
            Au::ZERO
        });
    }
    collect::shape(cx, b, &mut ifc, &widths);
    let (mut min, mut max) = (Au::ZERO, Au::ZERO);
    let (mut line, mut word, mut hang) = (Au::ZERO, Au::ZERO, Au::ZERO);
    for s in &ifc.segs {
        let a = if ifc.items[s.item].kind == ItemKind::Atomic {
            s.lead + crate::intrinsic::margin_box_widths(cx, ifc.items[s.item].b, cw).0 + s.trail
        } else {
            seg_width(s)
        };
        if s.kind == SegKind::Space {
            hang += seg_width(s);
        } else {
            hang = Au::ZERO;
            word += a;
        }
        line += seg_width(s);
        if s.after != After::Join {
            min = min.max(word);
            word = Au::ZERO;
        }
        if s.after == After::Mandatory {
            max = max.max(line - hang);
            (line, hang) = (Au::ZERO, Au::ZERO);
        }
    }
    let (mut min, mut max) = (min.max(word), max.max(line - hang));
    // floats sit side by side at most
    for &(_, f) in &ifc.floats {
        let (a, z) = crate::intrinsic::margin_box_widths(cx, f, cw);
        min = min.max(a);
        max += z;
    }
    (min, max)
}
