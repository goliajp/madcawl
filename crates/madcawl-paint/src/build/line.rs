//! Painting a line box's content. What is in a positioned or translucent inline box is painted with that
//! box, in its stacking layer, not with the line: the line paints only what no such box on it holds.

use madcawl_css::style::Visibility;
use madcawl_dom::NodeId;
use madcawl_layout::{FragKind, Fragment, Point};

use super::order::{Ctx, Unit};
use super::{Painter, boxes};
use crate::list::{Item, RectF};

// a piece of a non-atomic inline box: layout gives it no children
fn is_piece(f: &Fragment) -> bool {
    f.kind == FragKind::Box && f.children.is_empty()
}

fn piece_of(line: &Fragment, n: NodeId) -> bool {
    line.children
        .iter()
        .any(|c| c.frag.node == Some(n) && is_piece(&c.frag))
}

impl Painter<'_, '_, '_> {
    // whether a positioned or translucent inline box has a piece on the line
    fn has_layered_piece(&self, line: &Fragment) -> bool {
        line.children
            .iter()
            .any(|c| is_piece(&c.frag) && self.class_of(&c.frag).layered())
    }

    /// The innermost positioned or translucent inline box on `line` that `from` is or is in.
    fn owner_of(&self, line: &Fragment, mut from: Option<NodeId>) -> Option<NodeId> {
        while let Some(n) = from {
            if !piece_of(line, n) {
                return None;
            }
            if super::order::class(self.style(n)).layered() {
                return Some(n);
            }
            from = self.dom.parent(n);
        }
        None
    }

    /// The opacity of the inline boxes on `line` that `from` is or is in, multiplied.
    pub(super) fn chain_alpha(&self, line: &Fragment, mut from: Option<NodeId>) -> f32 {
        let mut a = 1.0;
        while let Some(n) = from.filter(|&n| piece_of(line, n)) {
            a *= self.style(n).map_or(1.0, |s| s.opacity());
            from = self.dom.parent(n);
        }
        a
    }

    /// The content of `line` that the inline box `owner` holds (the line's own content for `None`),
    /// under `cx`: the backgrounds and borders of the inline boxes, the text, then the atomic inlines, each
    /// painting as if it made a stacking context.
    pub(super) fn line(&mut self, line: &Unit, owner: Option<NodeId>, cx: Ctx) {
        let u = &line.frag;
        let check = owner.is_some() || self.has_layered_piece(u);
        let held = |p: &Self, from: Option<NodeId>| !check || p.owner_of(u, from) == owner;
        let place = |c: &madcawl_layout::Child| -> Point { line.at + c.offset };
        for c in u.children.iter() {
            let piece =
                is_piece(&c.frag) && c.frag.node.is_some_and(|n| !self.replaced.contains(&n));
            if piece && c.frag.node != owner && held(self, c.frag.node) {
                let p = self.unit_on(line, c, cx);
                self.own_box(&p);
            }
        }
        for c in u.children.iter() {
            let parent = c.frag.node.and_then(|n| self.dom.parent(n));
            if c.frag.kind == FragKind::Text && held(self, parent) {
                self.text(&c.frag, place(c), cx);
            }
        }
        for c in u.children.iter() {
            let Some(n) = c.frag.node else { continue };
            if c.frag.kind != FragKind::Box || self.class_of(&c.frag).layered() {
                continue;
            }
            if !held(self, self.dom.parent(n)) {
                continue;
            }
            let a = self.unit_on(line, c, cx);
            if self.replaced.contains(&n) {
                self.own_box(&a);
            } else if !c.frag.children.is_empty() {
                self.context(&a, false);
            }
        }
    }

    fn unit_on(&self, line: &Unit, c: &madcawl_layout::Child, cx: Ctx) -> Unit {
        Unit {
            frag: c.frag.clone(),
            at: line.at + c.offset,
            cx,
            line: Some((line.frag.clone(), line.at)),
        }
    }

    fn text(&mut self, frag: &Fragment, at: Point, cx: Ctx) {
        let Some(n) = frag.node else { return };
        let Some(s) = self.dom.parent(n).and_then(|p| self.style(p)) else {
            return;
        };
        if s.visibility != Visibility::Visible {
            return;
        }
        let Some(color) = boxes::text_color(s) else {
            return;
        };
        let color = color.fade(cx.alpha);
        let collapse = s.text.white_space_collapse;
        let (x, y) = (at.x.px(), at.y.px());
        let Some(size) = self.runs.run(self.layout, n, (x, y, frag.size.w), collapse) else {
            return;
        };
        // glyphs reach past the content area by up to about their size
        let bounds = RectF {
            x: x - size,
            y: y - size,
            w: frag.size.w.px() + 2.0 * size,
            h: frag.size.h.px() + 2.0 * size,
        };
        if !self.keep(bounds, cx.clip) {
            return;
        }
        let pieces = std::mem::take(&mut self.runs.pieces);
        for p in &pieces {
            let g = &self.runs.scratch[p.glyphs.clone()];
            if g.is_empty() {
                continue;
            }
            if !self.list.glyph_room(g.len()) {
                self.list.dropped += 1;
                continue;
            }
            let start = self.list.glyphs.len() as u32;
            self.list.glyphs.extend_from_slice(g);
            let item = Item::Glyphs {
                face: p.face,
                size,
                color,
                glyphs: start..self.list.glyphs.len() as u32,
            };
            self.list.push(item, bounds, cx.clip);
        }
        self.runs.pieces = pieces;
    }
}
