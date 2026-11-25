//! The last results of each box: a few (space, result) entries, the most recently used first. A flex item
//! is asked for in up to four spaces by its own container (its definite height or not, times measured or
//! stretched), and the entries hold those, so nested containers that ask the same spaces again do not
//! multiply the work by the nesting. They do not bound the work per box: each ancestor whose definite height
//! reaches the box can ask it for another containing block height, about three per ancestor through nested
//! columns, so a box at depth d may be laid out in some 3d spaces. Work is bounded by the distinct spaces
//! asked, quadratic in the depth and never exponential. A result laid out at the box's own size also answers
//! a space that fixes the box to that size, so a stretched item whose stretched size is the size it had is
//! not laid out again.

use madcawl_css::style::{ComputedStyle, Size as CssSize, Styles};
use madcawl_dom::{Dom, NodeKind};

use crate::cx::{Cx, Laid, Space};
use crate::style::Edges;
use crate::tree::{BoxTree, NONE};

const SLOTS: usize = 4;

pub type Slots = [Option<(Space, Laid)>; SLOTS];

pub const EMPTY: Slots = [const { None }; SLOTS];

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

/// The last result of `b` that answers `space`.
pub fn lookup(cx: &mut Cx, b: u32, space: &Space) -> Option<Laid> {
    let slots = &cx.cache[b as usize];
    let k = (0..SLOTS).find(|&k| {
        slots[k]
            .as_ref()
            .is_some_and(|(sp, laid)| answers(cx, b, sp, laid, space))
    })?;
    let laid = slots[k].as_ref().map(|(_, l)| l.clone());
    cx.cache[b as usize][..=k].rotate_right(1);
    laid
}

pub fn store(cx: &mut Cx, b: u32, space: Space, laid: &Laid) {
    let slots = &mut cx.cache[b as usize];
    // the same space replaces its entry; otherwise the least recently used one goes
    let k = slots
        .iter()
        .position(|e| e.as_ref().is_none_or(|(sp, _)| *sp == space))
        .unwrap_or(SLOTS - 1);
    slots[..=k].rotate_right(1);
    slots[0] = Some((space, laid.clone()));
}
