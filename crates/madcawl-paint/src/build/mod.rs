//! The fragment tree to a display list, in the paint order of CSS 2 appendix E: a stacking context paints
//! its own background and border, then its children with negative `z-index`, the backgrounds and borders of
//! its in-flow blocks in tree order, its floats, its inline content line by line, its positioned
//! descendants with `z-index: auto` or `0` in tree order, and last those with positive `z-index`. A float,
//! an inline block and a positioned box without `z-index` paint as if they made a stacking context, except
//! that their positioned descendants and the stacking contexts in them belong to the stacking context around
//! them. Boxes whose overflow is not `visible` clip their contents to their padding box.
//!
//! Opacity is applied to each item by itself, not to the group a stacking context paints: where items of
//! one translucent context overlap, the lower one shows through the upper one, which group opacity hides.

mod boxes;
mod line;
mod order;
mod text;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use madcawl_css::style::{ComputedStyle, Overflow, Styles, Visibility};
use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_layout::tree::Kind;
use madcawl_layout::{Au, FragKind, Fragment, Layout, Point};
use madcawl_str::atom;

use crate::list::{DisplayList, Item, RectF};

pub use order::Unit;

// how far below the area a line may start and still show: glyphs reach above their line box
const LINE_SLACK: f32 = 256.0;
use order::{Class, Ctx, Layers, Walk};

/// Builds the display list of `layout`'s document; only entries meeting `area` (CSS px on the page) are
/// kept when it is given.
pub fn paint(layout: &mut Layout, dom: &Dom, styles: &Styles, area: Option<RectF>) -> DisplayList {
    let Some(root) = layout.fragments().cloned() else {
        return DisplayList::default();
    };
    let tree = layout.boxes();
    let mut replaced = HashSet::new();
    for b in 0..tree.len() as u32 {
        let n = tree.get(b);
        if matches!(n.kind, Kind::Replaced { .. }) {
            replaced.insert(n.node);
        }
    }
    let runs = text::TextRuns::new(tree);
    let slices = slices(&root.frag, styles);
    let mut p = Painter {
        dom,
        styles,
        layout,
        list: DisplayList::default(),
        area,
        replaced,
        runs,
        slices,
        canvas_from: None,
    };
    p.canvas(root.frag.node);
    let unit = Unit {
        frag: root.frag,
        at: root.offset,
        cx: Ctx {
            clip: 0,
            alpha: 1.0,
        },
        line: None,
    };
    p.context(&unit, true);
    p.list
}

// the first and the last piece of each bordered inline box, in tree order
fn slices(root: &Arc<Fragment>, styles: &Styles) -> HashMap<NodeId, (usize, usize)> {
    let mut out = HashMap::new();
    root.walk(|_, f| {
        if f.kind != FragKind::Line {
            return;
        }
        for c in f.children.iter() {
            let Some(n) = c.frag.node.filter(|_| c.frag.children.is_empty()) else {
                continue;
            };
            if c.frag.kind != FragKind::Box || styles.get(n).is_none_or(|s| !boxes::bordered(s)) {
                continue;
            }
            let p = Arc::as_ptr(&c.frag) as usize;
            out.entry(n)
                .and_modify(|e: &mut (usize, usize)| e.1 = p)
                .or_insert((p, p));
        }
    });
    out
}

pub(crate) struct Painter<'a, 'l, 'd> {
    dom: &'d Dom,
    styles: &'d Styles,
    layout: &'a mut Layout<'l>,
    list: DisplayList,
    area: Option<RectF>,
    replaced: HashSet<NodeId>,
    runs: text::TextRuns,
    /// The first and the last piece of each inline box with a border, by address
    slices: HashMap<NodeId, (usize, usize)>,
    /// The element whose background went to the canvas, which it does not paint again
    canvas_from: Option<NodeId>,
}

fn rect(at: Point, f: &Fragment) -> RectF {
    RectF {
        x: at.x.px(),
        y: at.y.px(),
        w: f.size.w.px(),
        h: f.size.h.px(),
    }
}

impl Painter<'_, '_, '_> {
    fn style(&self, n: NodeId) -> Option<&ComputedStyle> {
        self.styles.get(n)
    }

    // the canvas takes the root element's background, or the body's when the root has none
    fn canvas(&mut self, root: Option<NodeId>) {
        let Some(root) = root else { return };
        let body = self.dom.children(root).find(|&c| {
            self.dom.kind(c) == NodeKind::Element && self.dom.local_name(c) == Some(atom!("body"))
        });
        for n in std::iter::once(root).chain(body) {
            if let Some(c) = self.style(n).and_then(boxes::background) {
                self.list.canvas = c;
                self.canvas_from = Some(n);
                return;
            }
        }
    }

    // whether something with `bounds` under `clip` can show in the area asked for
    fn keep(&mut self, bounds: RectF, clip: u32) -> bool {
        let clipped = bounds.intersect(&self.list.clips[clip as usize].rect);
        if clipped.is_empty() || self.area.is_some_and(|a| !a.intersects(&clipped)) {
            self.list.culled += 1;
            return false;
        }
        true
    }

    fn emit(&mut self, item: Item, bounds: RectF, clip: u32) {
        if self.keep(bounds, clip) {
            self.list.push(item, bounds, clip);
        }
    }

    /// The background and border of a box, unless it is hidden.
    fn own_box(&mut self, u: &Unit) {
        let Some(n) = u.frag.node else { return };
        let Some(s) = self.style(n) else { return };
        if s.visibility != Visibility::Visible {
            return;
        }
        let r = rect(u.at, &u.frag);
        let bg = boxes::background(s).filter(|_| self.canvas_from != Some(n));
        let border = boxes::border(s, r, u.cx.alpha);
        if let Some(c) = bg {
            let item = Item::Rect {
                rect: r,
                color: c.fade(u.cx.alpha),
            };
            self.emit(item, r, u.cx.clip);
        }
        if let Some(mut b) = border {
            if let Some(&(first, last)) = self.slices.get(&n) {
                let p = Arc::as_ptr(&u.frag) as usize;
                boxes::slice(&mut b, p == first, p == last);
            }
            self.emit(b, r, u.cx.clip);
        }
        if self.replaced.contains(&n) {
            self.emit(Item::Image { rect: r }, r, u.cx.clip);
        }
    }

    /// What a box's contents are painted under: its opacity, and its padding box as a clip when its
    /// overflow is not visible.
    fn inner(&mut self, frag: &Fragment, at: Point, cx: Ctx) -> Ctx {
        let Some(s) = frag.node.and_then(|n| self.style(n)) else {
            return cx;
        };
        let mut cx = cx;
        if s.place.overflow_x != Overflow::Visible || s.place.overflow_y != Overflow::Visible {
            let b = s.boxm.border_width.map(|w| Au(w).px());
            let r = rect(at, frag);
            let pad = RectF {
                x: r.x + b[3],
                y: r.y + b[0],
                w: r.w - b[1] - b[3],
                h: r.h - b[0] - b[2],
            };
            cx.clip = self.list.clip(cx.clip, pad);
        }
        cx
    }

    /// Paints a stacking context (`real`), or a box that paints as if it made one, with what belongs to it;
    /// a piece of an inline box paints itself and what it holds on its line.
    fn context(&mut self, u: &Unit, real: bool) {
        let alpha = u
            .frag
            .node
            .and_then(|n| self.style(n))
            .map_or(1.0, |s| s.opacity());
        if alpha == 0.0 {
            return;
        }
        let u = Unit {
            cx: Ctx {
                alpha: u.cx.alpha * alpha,
                ..u.cx
            },
            ..u.clone()
        };
        self.own_box(&u);
        if let Some((line, at)) = &u.line
            && u.frag.children.is_empty()
        {
            let line = Unit {
                frag: line.clone(),
                at: *at,
                cx: u.cx,
                line: None,
            };
            self.line(&line, u.frag.node, u.cx);
            return;
        }
        let inner = self.inner(&u.frag, u.at, u.cx);
        let mut l = Layers::default();
        let walk = if real { Walk::Context } else { Walk::Own };
        self.gather(&u, inner, walk, &mut l);
        l.sort();
        for c in &l.negative {
            self.context(&c.1, true);
        }
        for b in &l.blocks {
            self.own_box(b);
        }
        for f in &l.floats {
            self.context(f, false);
        }
        for line in &l.lines {
            // the lines of a context go down the page, so one below the area is after every line of its
            // text nodes that can show; leaving it out leaves the text of the others where it was
            if self
                .area
                .is_some_and(|a| line.at.y.px() > a.bottom() + LINE_SLACK)
            {
                continue;
            }
            self.line(line, None, line.cx);
        }
        for c in &l.flow {
            let real = matches!(self.class_of(&c.frag), Class::Context(_));
            self.context(c, real);
        }
        for c in &l.positive {
            self.context(&c.1, true);
        }
    }
}
