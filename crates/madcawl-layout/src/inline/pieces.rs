//! The fragments of one line: its text runs, the pieces of the inline boxes on it, and empty inline boxes;
//! which inline boxes a line crosses, and where the out-of-flow boxes among its text are.

use madcawl_text::Kind as SegKind;

use super::LineRange;
use super::collect::ItemKind;
use super::line::Builder;
use super::line::valign_shift;
use crate::au::{Au, Point, Size};
use crate::cx::Cx;
use crate::fragment::{Child, FragKind, Fragment};
use crate::style::{Edges, line_height};
use crate::tree::Kind;
use madcawl_css::style::{Overflow, VAlign, VerticalAlign};

impl Builder<'_> {
    // an atomic's top margin edge from its parent's baseline, and its margin box height
    pub(super) fn atomic_place(&self, cx: &mut Cx, item: usize) -> (Au, Au) {
        let r = self.laid[item]
            .as_ref()
            .expect("an atomic item is laid out");
        let b = self.ifc.items[item].b;
        let s = cx.style(b);
        let e = Edges::of(s, self.inner.width);
        let h = e.m(0) + r.frag.size.h + e.m(2);
        let own =
            cx.tree.get(b).kind == Kind::InlineBlock && s.place.overflow_y == Overflow::Visible;
        let bl = match r.baseline {
            Some(bl) if own => e.m(0) + bl,
            _ => h,
        };
        let p = self.parent_box(item);
        let top = match s.rare.vertical_align {
            // its middle at the parent's baseline raised by half the x-height
            VAlign::Kw(VerticalAlign::Middle) => -h.half() - p.x_height.half(),
            VAlign::Kw(VerticalAlign::TextTop) => -p.asc,
            VAlign::Kw(VerticalAlign::TextBottom) => p.desc - h,
            v => {
                let f = cx.font(b);
                valign_shift(v, p.size, line_height(s, &f)) - bl
            }
        };
        (top, h)
    }

    // the inline boxes the line from `span` crosses, and the empty ones on it, in tree order; boxes start in
    // tree order and lines come in text order, so a box is taken up once and dropped once the lines pass it
    pub(super) fn boxes_on(
        &mut self,
        cx: &mut Cx,
        span: std::ops::Range<usize>,
        last_line: bool,
    ) -> (Vec<usize>, Vec<usize>) {
        let boxes = &self.ifc.boxes;
        while let Some(b) = boxes.get(self.next_box)
            && (b.range.start < span.end || last_line && b.range.start == span.end)
        {
            self.active.push(self.next_box);
            self.next_box += 1;
        }
        cx.counters.inline_box_visits += self.active.len() as u64;
        let (mut on_line, mut empties) = (Vec::new(), Vec::new());
        for &k in &self.active {
            let br = &boxes[k].range;
            if br.is_empty() {
                if br.start >= span.start {
                    empties.push(k);
                }
            } else if br.start < span.end && br.end > span.start {
                on_line.push(k);
            }
        }
        self.active.retain(|&k| boxes[k].range.end > span.end);
        (on_line, empties)
    }

    // the static positions of the out-of-flow boxes among the line's text: where the text after them starts,
    // at the line's top
    pub(super) fn out_of_flow(
        &mut self,
        segs: &[super::collect::Seg],
        xs: &[(Au, Au, Au)],
        last_line: bool,
    ) {
        let end = segs.last().map_or(0, |s| s.range.end);
        let line_end =
            xs.last().map_or(Au::ZERO, |x| x.2) + segs.last().map_or(Au::ZERO, |s| s.trail);
        while let Some(&(off, a)) = self.ifc.abs.get(self.next_abs)
            && (off < end || last_line && off == end)
        {
            let k = segs.partition_point(|s| s.range.start < off);
            let x = xs.get(k).map_or(line_end, |x| x.0);
            self.out.statics.push((a, Point { x, y: self.y }));
            self.next_abs += 1;
        }
    }

    pub(super) fn text_runs(
        &self,
        r: &LineRange,
        xs: &[(Au, Au, Au)],
        baseline: Au,
        kids: &mut Vec<Child>,
    ) {
        let segs = &self.ifc.segs[r.segs.clone()];
        let mut k = 0;
        while k < segs.len() {
            let (item, start) = (segs[k].item, k);
            while k < segs.len() && segs[k].item == item {
                k += 1;
            }
            let it = &self.ifc.items[item];
            let p = self.parent_box(item);
            let y = baseline + self.item_shift[item] - p.asc;
            if it.kind == ItemKind::Break {
                let size = Size {
                    w: Au::ZERO,
                    h: p.asc + p.desc,
                };
                kids.push(Child {
                    offset: Point { x: xs[start].1, y },
                    frag: Fragment::new(FragKind::Box, Some(it.node), size, Vec::new()),
                });
            }
            if it.kind != ItemKind::Text {
                continue;
            }
            let size = Size {
                w: xs[k - 1].2 - xs[start].1,
                h: p.asc + p.desc,
            };
            kids.push(Child {
                offset: Point {
                    x: xs[start].1,
                    y: baseline + self.item_shift[item] - p.asc,
                },
                frag: Fragment::new(FragKind::Text, Some(it.node), size, Vec::new()),
            });
        }
    }

    // the leads of the boxes starting at segment `seg` outside box `k`, and the trails of those ending
    // there inside it
    fn edges_around(&self, seg: usize, k: usize, on_line: &[usize]) -> (Au, Au) {
        let (mut lead, mut trail) = (Au::ZERO, Au::ZERO);
        for &j in on_line {
            let o = &self.ifc.boxes[j];
            let Some((f, l)) = o.segs else { continue };
            if j < k && f == seg {
                lead += o.start.0 + o.start.1;
            }
            if j > k && l == seg {
                trail += o.end.0 + o.end.1;
            }
        }
        (lead, trail)
    }

    pub(super) fn box_pieces(
        &self,
        r: &LineRange,
        xs: &[(Au, Au, Au)],
        baseline: Au,
        on_line: &[usize],
        kids: &mut Vec<Child>,
    ) {
        let base = r.segs.start;
        // white space hanging at the line's end is in no box piece
        let segs = &self.ifc.segs[r.segs.clone()];
        let end = base
            + segs
                .iter()
                .rposition(|s| s.kind != SegKind::Space)
                .unwrap_or(0)
            + 1;
        for &k in on_line {
            let ib = &self.ifc.boxes[k];
            let Some((f, l)) = ib.segs else { continue };
            let first = f.max(base) - base;
            let last = l.min(end - 1).max(f.max(base)) - base;
            let left = if f >= base {
                xs[first].0 + self.edges_around(f, k, on_line).0 + ib.start.0
            } else {
                xs[first].1
            };
            let right = if l < r.segs.end {
                xs[last].2 + self.edges_around(l, k, on_line).1 + ib.end.1
            } else {
                xs[last].2
            };
            let v = self.boxes[k];
            let size = Size {
                w: right - left,
                h: v.asc + v.desc + v.bp_top + v.bp_bottom,
            };
            kids.push(Child {
                offset: Point {
                    x: left,
                    y: baseline + v.shift - v.asc - v.bp_top,
                },
                frag: Fragment::new(FragKind::Box, Some(ib.node), size, Vec::new()),
            });
        }
    }

    // inline boxes with nothing in them: a piece as wide as their own borders and padding where they are
    pub(super) fn empty_boxes(
        &self,
        r: &LineRange,
        xs: &[(Au, Au, Au)],
        baseline: Au,
        empties: &[usize],
        kids: &mut Vec<Child>,
    ) {
        let segs = &self.ifc.segs[r.segs.clone()];
        for &k in empties {
            let ib = &self.ifc.boxes[k];
            let x = match segs.iter().position(|s| s.range.start >= ib.range.start) {
                Some(i) => xs[i].0,
                None => xs.last().map_or(Au::ZERO, |x| x.2),
            };
            let v = self.boxes[k];
            let size = Size {
                w: ib.start.1 + ib.end.1,
                h: v.asc + v.desc + v.bp_top + v.bp_bottom,
            };
            kids.push(Child {
                offset: Point {
                    x: x + ib.start.0,
                    y: baseline + v.shift - v.asc - v.bp_top,
                },
                frag: Fragment::new(FragKind::Box, Some(ib.node), size, Vec::new()),
            });
        }
    }
}

// each segment's x before its lead, after its lead, and after its advance
pub(super) fn positions(segs: &[super::collect::Seg], x0: Au) -> Vec<(Au, Au, Au)> {
    let mut x = x0;
    segs.iter()
        .map(|s| {
            let before = x;
            let start = before + s.lead;
            x = start + s.adv + s.trail;
            (before, start, start + s.adv)
        })
        .collect()
}
