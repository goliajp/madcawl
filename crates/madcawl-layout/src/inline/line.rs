//! Line boxes: each line's height from the root inline box's strut, the inline boxes and the atomic inlines
//! on it, aligned on one baseline; then the fragments of its text runs, inline box pieces and atomics.

use std::collections::HashMap;

use madcawl_css::style::{Direction, TextAlign, VAlign, VerticalAlign};
use madcawl_text::Kind as SegKind;

use super::collect::{Ifc, ItemKind};
use super::{LineRange, Lines};
use crate::au::{Au, Point, Size};
use crate::cx::{Cx, Laid, Space};
use crate::fragment::{Child, FragKind, Fragment};

use super::pieces::positions;
use super::shift::{valign_shift, vbox};
use crate::style::line_height;

/// An inline box's vertical metrics: its baseline's shift from the root's, the extent of its line height
/// above and below that baseline, and of its content area plus borders and padding.
#[derive(Clone, Copy, Default)]
pub(super) struct VBox {
    pub(super) shift: Au,
    pub(super) above: Au,
    pub(super) below: Au,
    pub(super) asc: Au,
    pub(super) desc: Au,
    pub(super) size: f32,
    pub(super) x_height: Au,
    pub(super) bp_top: Au,
    pub(super) bp_bottom: Au,
    /// It has inline margins, borders or padding: a line with it is not empty
    pub(super) edged: bool,
    /// Its relative offset and its ancestors'
    pub(super) rel: Point,
    /// The nearest box around it or itself aligned with the line's top (true) or bottom
    pub(super) edge: Option<(usize, bool)>,
}

pub struct Builder<'i> {
    pub(super) ifc: &'i Ifc,
    pub(super) laid: &'i [Option<Laid>],
    pub(super) inner: Space,
    pub(super) strut: VBox,
    pub(super) align: TextAlign,
    pub(super) rtl: bool,
    pub(super) boxes: Vec<VBox>,
    /// By tree box index, its entry in `boxes`
    pub(super) index: HashMap<u32, usize>,
    /// By item, the baseline shift of what it is in
    pub(super) item_shift: Vec<Au>,
    pub(super) out: Lines,
    pub(super) y: Au,
    /// The next inline box no line has reached, and those reached that later lines may still cross
    pub(super) next_box: usize,
    pub(super) active: Vec<usize>,
    /// The next out-of-flow box no line has reached
    pub(super) next_abs: usize,
    /// By box, how far the current line moves it to the line's top or bottom
    pub(super) edge_dy: Vec<Au>,
}

impl<'i> Builder<'i> {
    pub fn new(
        cx: &mut Cx,
        root: u32,
        ifc: &'i Ifc,
        laid: &'i [Option<Laid>],
        inner: Space,
    ) -> Self {
        let rf = cx.font(root);
        let rs = cx.style(root);
        let strut = vbox(&rf, line_height(rs, &rf));
        let mut index = HashMap::new();
        let mut boxes: Vec<VBox> = Vec::with_capacity(ifc.boxes.len());
        for (k, ib) in ifc.boxes.iter().enumerate() {
            let pv = index.get(&ib.parent).map_or(strut, |&p: &usize| boxes[p]);
            let s = cx.style(ib.b);
            let f = cx.font(ib.b);
            let lh = line_height(s, &f);
            let e = cx.edges(s, inner.width);
            let edge = match s.rare.vertical_align {
                VAlign::Kw(VerticalAlign::Top) => Some((k, true)),
                VAlign::Kw(VerticalAlign::Bottom) => Some((k, false)),
                _ => pv.edge,
            };
            let cb = Space::new(inner.width, None);
            boxes.push(VBox {
                shift: pv.shift + valign_shift(s.rare.vertical_align, pv.size, lh),
                bp_top: e.bp(0),
                bp_bottom: e.bp(2),
                edged: ib.start.0 + ib.start.1 + ib.end.0 + ib.end.1 != Au::ZERO,
                rel: pv.rel + crate::position::relative_offset(s, cb, true),
                edge,
                ..vbox(&f, lh)
            });
            index.insert(ib.b, k);
        }
        let item_shift = ifc
            .items
            .iter()
            .map(|it| index.get(&it.parent).map_or(Au::ZERO, |&p| boxes[p].shift))
            .collect();
        Builder {
            ifc,
            laid,
            inner,
            strut,
            align: rs.text.text_align,
            rtl: rs.text.direction == Direction::Rtl,
            boxes,
            index,
            item_shift,
            out: Lines {
                lines: Vec::new(),
                height: Au::ZERO,
                baseline: None,
                empty: true,
                statics: Vec::new(),
            },
            y: Au::ZERO,
            next_box: 0,
            active: Vec::new(),
            next_abs: 0,
            edge_dy: vec![Au::ZERO; ifc.boxes.len()],
        }
    }

    fn offset(&self, free: Au) -> Au {
        use TextAlign::*;
        let free = free.max(Au::ZERO);
        match (self.align, self.rtl) {
            (Right | WebkitRight, _) | (End, false) | (Start, true) => free,
            (Center | WebkitCenter, _) => free.half(),
            _ => Au::ZERO,
        }
    }

    pub(super) fn parent_box(&self, item: usize) -> VBox {
        let p = self.ifc.items[item].parent;
        self.index.get(&p).map_or(self.strut, |&k| self.boxes[k])
    }

    pub fn y(&self) -> Au {
        self.y
    }

    /// Where the next line starts, below floats the last one did not fit beside.
    pub fn move_to(&mut self, y: Au) {
        self.y = self.y.max(y);
    }

    pub fn strut_height(&self) -> Au {
        self.strut.above + self.strut.below
    }

    /// Lays out line `r` from `left` in the room of `avail`.
    pub fn line(&mut self, cx: &mut Cx, r: &LineRange, left: Au, avail: Au) {
        let segs = &self.ifc.segs[r.segs.clone()];
        let (Some(first), Some(last)) = (segs.first(), segs.last()) else {
            return;
        };
        let span = first.range.start..last.range.end;
        let (mut top, mut bottom) = (-self.strut.above, self.strut.below);
        let mut filled = false;
        let last_line = r.segs.end == self.ifc.segs.len();
        let (on_line, empties) = self.boxes_on(cx, span.clone(), last_line);
        for &k in on_line.iter().chain(&empties) {
            let v = self.boxes[k];
            if v.edge.is_none() {
                top = top.min(v.shift - v.above);
                bottom = bottom.max(v.shift + v.below);
            }
            filled |= v.edged;
        }
        let mut atomics = Vec::new();
        // atomics aligned with the line's top or bottom: placed once the rest has given the line its extent,
        // which they stretch down or up when taller
        let mut edge = Vec::new();
        for (k, s) in segs.iter().enumerate() {
            match self.ifc.items[s.item].kind {
                ItemKind::Atomic => {
                    let (t, h) = self.atomic_place(cx, s.item);
                    let t = t + self.item_shift[s.item];
                    match cx.style(self.ifc.items[s.item].b).rare.vertical_align {
                        VAlign::Kw(VerticalAlign::Top) => edge.push((atomics.len(), h, true)),
                        VAlign::Kw(VerticalAlign::Bottom) => edge.push((atomics.len(), h, false)),
                        _ if self.item_aligned(s.item) => {}
                        _ => {
                            top = top.min(t);
                            bottom = bottom.max(t + h);
                        }
                    }
                    atomics.push((k, s.item, t));
                    filled = true;
                }
                ItemKind::Break => filled = true,
                ItemKind::Text => {
                    filled |= s.kind == SegKind::Text || self.ifc.items[s.item].preserve;
                    if let Some((a, d)) = s.fallback
                        && !self.item_aligned(s.item)
                    {
                        let shift = self.item_shift[s.item];
                        top = top.min(shift - a);
                        bottom = bottom.max(shift + d);
                    }
                }
            }
        }
        if !filled {
            return;
        }
        for &(_, h, at_top) in &edge {
            if at_top {
                bottom = bottom.max(top + h);
            } else {
                top = top.min(bottom - h);
            }
        }
        let all: Vec<usize> = on_line.iter().chain(&empties).copied().collect();
        self.align_edges(&all, &mut top, &mut bottom);
        for &(a, h, at_top) in &edge {
            atomics[a].2 = if at_top { top } else { bottom - h };
        }
        self.out.empty = false;
        let (height, baseline) = (bottom - top, -top);
        let x0 = left + self.offset(avail - r.width);
        let xs = positions(segs, x0);
        let mut kids = Vec::new();
        self.text_runs(r, &xs, baseline, &mut kids);
        self.box_pieces(r, &xs, baseline, &on_line, &mut kids);
        self.empty_boxes(r, &xs, baseline, &empties, &mut kids);
        let line_at = Point {
            x: Au::ZERO,
            y: self.y,
        };
        for (k, item, t) in atomics {
            let laid = self.laid[item]
                .as_ref()
                .expect("an atomic item is laid out");
            let mt = cx
                .edges(cx.style(self.ifc.items[item].b), self.inner.width)
                .m(0);
            let off = self.item_off(item);
            let offset = Point {
                x: xs[k].1 + laid.ml + off.x,
                y: baseline + t + mt + off.y,
            };
            let st = laid.statics.iter().map(|&(a, p)| (a, p + offset + line_at));
            self.out.statics.extend(st);
            kids.push(Child {
                offset,
                frag: laid.frag.clone(),
            });
        }
        self.out_of_flow(segs, &xs, last_line);
        let size = Size {
            w: self.inner.width,
            h: height,
        };
        self.out.lines.push(Child {
            offset: Point {
                x: Au::ZERO,
                y: self.y,
            },
            frag: Fragment::new(FragKind::Line, None, size, kids),
        });
        self.reset_edges(&all);
        self.out.baseline = Some(self.y + baseline);
        self.y += height;
    }

    pub fn finish(mut self) -> Lines {
        for &(_, a) in &self.ifc.abs[self.next_abs..] {
            self.out.statics.push((
                a,
                Point {
                    x: Au::ZERO,
                    y: self.y,
                },
            ));
        }
        self.out.height = self.y;
        self.out
    }
}
