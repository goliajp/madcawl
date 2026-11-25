//! The cross sizes of a flex line's items: each laid out at its flexed main size, the line's cross size from
//! them, and the items that stretch laid out again at that size.

use crate::au::Au;
use crate::cx::{Cx, Space};
use crate::{block, cache};

use super::item::Item;

/// Whether `it` is laid out only for its size: its container is being measured, or it stretches and is
/// laid out again. Without reuse every item is laid out in full, the plain algorithm reuse is checked
/// against.
pub(super) fn measures(cx: &Cx, it: &Item) -> bool {
    cx.reuse && (cx.measuring || it.stretch)
}

// lays each item out at its flexed main size; the line's cross size, which a stretched item then takes
pub(super) fn cross_sizes(
    cx: &mut Cx,
    items: &mut [Item],
    is_row: bool,
    inner: Space,
    (fixed, clamp): (Option<Au>, Option<(Au, Au)>),
) -> Au {
    let mut line = Au::ZERO;
    for it in items.iter_mut() {
        let r = cx.measure(measures(cx, it), |cx| {
            block::layout(cx, it.b, space(it, is_row, inner, it.fit))
        });
        it.cross = if is_row { r.frag.size.h } else { r.frag.size.w };
        line = line.max(
            it.cross
                + if is_row {
                    it.e.m(0) + it.e.m(2)
                } else {
                    it.e.m(1) + it.e.m(3)
                },
        );
        it.laid = Some(r);
    }
    let line = fixed.unwrap_or(match clamp {
        Some((lo, hi)) => line.clamp_lo_hi(lo, hi),
        None => line,
    });
    for it in items.iter_mut().filter(|it| it.stretch) {
        let sp = space(
            it,
            is_row,
            inner,
            Some((line - it.outer_cross).max(Au::ZERO)),
        );
        // stretching changes no line's size: measuring takes only a result it already has
        let r = if !cx.measuring {
            block::layout(cx, it.b, sp)
        } else if let Some(r) = cache::reusable(cx, it.b, &sp) {
            r
        } else {
            cx.inexact += 1;
            continue;
        };
        it.cross = if is_row { r.frag.size.h } else { r.frag.size.w };
        it.laid = Some(r);
    }
    line
}

fn space(it: &Item, is_row: bool, inner: Space, cross: Option<Au>) -> Space {
    let mut sp = Space::new(inner.width, inner.height);
    if is_row {
        sp.fixed = [Some(it.main), cross];
    } else {
        sp.fixed = [cross, Some(it.main)];
    }
    sp
}
