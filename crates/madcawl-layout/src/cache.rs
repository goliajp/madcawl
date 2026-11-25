//! The last results of each box: a few (space, result) entries, the most recently used first. A flex item
//! is asked for in up to four spaces by its own container (its definite height or not, times measured or
//! stretched), and the entries hold those, so nested containers that ask the same spaces again do not
//! multiply the work by the nesting. They do not bound the work per box: each ancestor whose definite height
//! reaches the box can ask it for another containing block height, about three per ancestor through nested
//! columns, so a box at depth d may be laid out in some 3d spaces. Work is bounded by the distinct spaces
//! asked, quadratic in the depth and never exponential. A result laid out at the box's own size also answers
//! a space that fixes the box to that size, so a stretched item whose stretched size is the size it had is
//! not laid out again. Results laid out while a flex container only measures its items, with the stretching
//! inside them left out, are kept apart from the exact ones and answer only further measuring, so measuring
//! and laying out the same item do not each lay out everything below it again.

use madcawl_css::style::{ComputedStyle, Size as CssSize, Styles};
use madcawl_dom::{Dom, NodeKind};

use crate::cx::{Cx, Laid, Space};
use crate::style::Edges;
use crate::tree::{BoxTree, NONE};

const SLOTS: usize = 4;

type Entry = Option<(Space, Laid)>;

/// A box's entries, the most recently used first. Most boxes are laid out in one space only, so the first
/// entry is held in place and the others are allocated when a second one is stored: four in place would be
/// most of a large document's layout memory.
#[derive(Clone)]
pub struct Slots {
    first: Entry,
    rest: Option<Box<[Entry; SLOTS - 1]>>,
}

pub const EMPTY: Slots = Slots { first: None, rest: None };

impl Slots {
    fn get(&self, k: usize) -> Option<&(Space, Laid)> {
        match k {
            0 => self.first.as_ref(),
            _ => self.rest.as_ref()?[k - 1].as_ref(),
        }
    }

    /// Moves entry `k` to the front, those before it back by one.
    fn bring_forward(&mut self, k: usize) {
        if k == 0 {
            return;
        }
        let rest = self.rest.as_mut().expect("an entry after the first is in the rest");
        rest[..k].rotate_right(1);
        std::mem::swap(&mut self.first, &mut rest[0]);
    }

    /// Puts `e` in front of the first `k` entries, dropping entry `k`.
    fn push_front(&mut self, k: usize, e: (Space, Laid)) {
        if k > 0 {
            let rest = self.rest.get_or_insert_with(|| Box::new([const { None }; SLOTS - 1]));
            rest[..k].rotate_right(1);
            rest[0] = self.first.take();
        }
        self.first = Some(e);
    }
}

fn pct(s: CssSize) -> bool {
    matches!(s, CssSize::Lp(l) if l.has_pct())
}

// whether a box's own layout reads the height of its containing block: a percentage height or limit, a
// vertical inset, a percentage row gap or flex basis
fn reads_height(s: &ComputedStyle) -> bool {
    let b = &s.boxm;
    let i = &s.place.inset;
    pct(b.height)
        || pct(b.min_height)
        || pct(b.max_height)
        || pct(i[0])
        || pct(i[2])
        || pct(s.rare.flex_basis)
        || s.rare.row_gap.is_some_and(|g| g.has_pct())
}

fn element_style<'s>(
    dom: &Dom,
    styles: &'s Styles,
    tree: &BoxTree,
    b: u32,
) -> Option<&'s ComputedStyle> {
    let bx = tree.get(b);
    if bx.pseudo != 0 {
        return styles.pseudo(bx.node, bx.pseudo == 2);
    }
    let n = bx.node;
    let el = if dom.kind(n) == NodeKind::Element {
        Some(n)
    } else {
        dom.parent(n)
    };
    el.and_then(|e| styles.get(e))
}

/// By box, whether it or a box below it reads a definite height it is given: the boxes a different
/// containing block height can change.
pub fn height_readers(dom: &Dom, styles: &Styles, tree: &BoxTree) -> Vec<bool> {
    let mut out = vec![false; tree.len()];
    for b in 0..tree.len() as u32 {
        mark(dom, styles, tree, &mut out, b);
    }
    out
}

/// Marks `b` and its ancestors when its style reads the height it is given; marks are never cleared, which
/// only costs reuse.
pub fn mark(dom: &Dom, styles: &Styles, tree: &BoxTree, out: &mut [bool], b: u32) {
    if !element_style(dom, styles, tree, b).is_some_and(reads_height) {
        return;
    }
    let mut up = b;
    while up != NONE && !out[up as usize] {
        out[up as usize] = true;
        up = tree.get(up).parent;
    }
}

// whether the result `laid` of `b` in `have` is what laying it out in `want` would give
fn answers(cx: &Cx, b: u32, have: &Space, laid: &Laid, want: &Space) -> bool {
    if have == want {
        return true;
    }
    if have.width != want.width {
        return false;
    }
    let reads = cx.height_readers[b as usize];
    if have.height != want.height && reads {
        return false;
    }
    let e = if cx.has_edges(b) {
        cx.edges(cx.style(b), have.width)
    } else {
        Edges::default()
    };
    let natural = [
        laid.frag.size.w - e.bp_inline(),
        laid.frag.size.h - e.bp_block(),
    ];
    (0..2).all(|k| match (have.fixed[k], want.fixed[k]) {
        (a, w) if a == w => true,
        // a definite height its contents may resolve percentages against
        // the fixed height is a definite one: a box whose contents read it, or whose own minimum or maximum
        // made its auto height (a flex container then fills or overflows a definite height), lays out
        // differently. its bottom margin and whether margins collapse through it also differ under a fixed
        // height, but only flex items and absolutely positioned boxes are given one, and neither has margins that
        // collapse with its parent's
        (None, Some(v)) => v == natural[k] && (k == 0 || !(reads || laid.clamped)),
        _ => false,
    })
}

// the last result in `b`'s exact or measured entries that answers `space`
fn find(cx: &mut Cx, measured: bool, b: u32, space: &Space) -> Option<Laid> {
    let slots = if measured {
        &cx.measures[b as usize]
    } else {
        &cx.cache[b as usize]
    };
    let k = (0..SLOTS).find(|&k| slots.get(k).is_some_and(|(sp, laid)| answers(cx, b, sp, laid, space)))?;
    let laid = slots.get(k).map(|(_, l)| l.clone());
    let slots = if measured {
        &mut cx.measures[b as usize]
    } else {
        &mut cx.cache[b as usize]
    };
    slots.bring_forward(k);
    laid
}

/// The last result of `b` that answers `space`: an exact one, or while measuring one that holds only its
/// size, which leaves what is laid out with it inexact too.
pub fn lookup(cx: &mut Cx, b: u32, space: &Space) -> Option<Laid> {
    if let Some(laid) = find(cx, false, b, space) {
        return Some(laid);
    }
    if !cx.measuring {
        return None;
    }
    let laid = find(cx, true, b, space)?;
    cx.inexact += 1;
    Some(laid)
}

/// The last result of `b` that may stand in for laying it out in `space`: reuse is on and floats outside it
/// had no say in either.
pub fn reusable(cx: &mut Cx, b: u32, space: &Space) -> Option<Laid> {
    if !cx.reuse || cx.excl.last().is_some_and(|e| !e.is_empty()) {
        return None;
    }
    let inexact = cx.inexact;
    let laid = lookup(cx, b, space)?;
    if laid.floats {
        cx.inexact = inexact;
        return None;
    }
    cx.counters.reused += 1;
    Some(laid)
}

/// Keeps `laid` as `b`'s result in `space`, with the exact ones or with those holding only a size.
pub fn store(cx: &mut Cx, b: u32, space: Space, laid: &Laid, exact: bool) {
    let slots = if exact {
        &mut cx.cache[b as usize]
    } else {
        &mut cx.measures[b as usize]
    };
    // the same space replaces its entry; otherwise the least recently used one goes
    let k = (0..SLOTS)
        .position(|k| slots.get(k).is_none_or(|(sp, _)| *sp == space))
        .unwrap_or(SLOTS - 1);
    slots.push_front(k, (space, laid.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::au::{Au, Size};
    use crate::cx::Strut;
    use crate::fragment::{FragKind, Fragment};

    fn entry(w: i32) -> (Space, Laid) {
        let frag = Fragment::new(FragKind::Box, None, Size::default(), Vec::new());
        let laid = Laid {
            frag,
            mt: Strut::default(),
            mb: Strut::default(),
            ml: Au::default(),
            mr: Au::default(),
            through: false,
            baseline: None,
            clamped: false,
            floats: false,
            statics: Vec::new(),
        };
        (Space::new(Au::from_px(w as f32), None), laid)
    }

    fn widths(s: &Slots) -> Vec<Option<i32>> {
        (0..SLOTS).map(|k| s.get(k).map(|(sp, _)| sp.width.px() as i32)).collect()
    }

    // the entries kept in place and in the rest move as the four in an array did
    #[test]
    fn slots_move_as_an_array_of_four() {
        let mut s = EMPTY;
        let mut model: [Option<i32>; SLOTS] = [None; SLOTS];
        let mut x = 7u32;
        for _ in 0..2000 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let w = (x >> 16) as i32 % 6;
            if x & 1 == 0 {
                let k = (0..SLOTS).position(|k| model[k].is_none_or(|m| m == w)).unwrap_or(SLOTS - 1);
                model[..=k].rotate_right(1);
                model[0] = Some(w);
                let k2 = (0..SLOTS).position(|k| s.get(k).is_none_or(|(sp, _)| sp.width.px() as i32 == w));
                s.push_front(k2.unwrap_or(SLOTS - 1), entry(w));
            } else if let Some(k) = (0..SLOTS).find(|&k| model[k] == Some(w)) {
                model[..=k].rotate_right(1);
                s.bring_forward(k);
            }
            assert_eq!(widths(&s), model.to_vec());
        }
    }

    #[test]
    fn one_entry_allocates_nothing_more() {
        let mut s = EMPTY;
        s.push_front(0, entry(3));
        s.push_front(0, entry(4));
        assert!(s.rest.is_none());
        s.push_front(1, entry(5));
        assert!(s.rest.is_some());
        assert_eq!(widths(&s), vec![Some(5), Some(4), None, None]);
    }
}
