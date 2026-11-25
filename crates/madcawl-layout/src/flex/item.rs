//! Flex items: their flex base sizes and limits, the lines they break into, and their flexible lengths
//! resolved (CSS Flexbox 9.2 to 9.7).

use madcawl_css::style::{ComputedStyle, Overflow, SizeKw};

use crate::au::Au;
use crate::block;
use crate::cx::{Cx, Laid, Space};
use crate::style::{Edges, content_size, size};
use crate::{intrinsic, position};

pub(super) struct Item {
    pub(super) b: u32,
    pub(super) e: Edges,
    /// Margins, borders and padding on the main axis, and on the cross axis
    pub(super) outer_main: Au,
    pub(super) outer_cross: Au,
    pub(super) base: Au,
    pub(super) min: Au,
    pub(super) max: Au,
    pub(super) grow: f32,
    pub(super) shrink: f32,
    pub(super) main: Au,
    pub(super) frozen: bool,
    pub(super) laid: Option<Laid>,
    pub(super) cross: Au,
    /// Across a column, the width of an item that does not stretch: it fits its content
    pub(super) fit: Option<Au>,
}

/// The computed values a flex item takes: an anonymous item has the initial ones, not its container's.
pub(super) fn item_style<'a>(cx: &Cx<'a>, c: u32) -> &'a ComputedStyle {
    if cx.has_edges(c) {
        cx.style(c)
    } else {
        ComputedStyle::initial()
    }
}

/// Whether `b` is an item of a flex container, which starts a formatting context of its own.
pub fn is_item(cx: &Cx, b: u32) -> bool {
    let p = cx.tree.get(b).parent;
    p != crate::tree::NONE && cx.has_edges(p) && super::is_flex(cx.style(p))
}

// the item's content-box size along the main axis that `flex-basis` and the size properties give, else
// its content's
fn base_size(cx: &mut Cx, it: &Item, s: &ComputedStyle, is_row: bool, inner: Space) -> Au {
    let (bp, prop, base) = if is_row {
        (it.e.bp_inline(), s.boxm.width, Some(inner.width))
    } else {
        (it.e.bp_block(), s.boxm.height, inner.height)
    };
    let basis = match s.rare.flex_basis {
        madcawl_css::style::Size::Kw(SizeKw::Auto) => prop,
        v => v,
    };
    if let Some(v) = size(basis, base) {
        return content_size(s, v, bp);
    }
    if is_row {
        intrinsic::content_widths(cx, it.b, inner.width).1
    } else {
        // measured as it is laid out later, so that pass finds this result
        let mut sp = Space::new(inner.width, inner.height);
        sp.fixed[0] = it.fit;
        let r = block::layout(cx, it.b, sp);
        r.frag.size.h - bp
    }
}

pub(super) fn item(cx: &mut Cx, b: u32, is_row: bool, inner: Space, fit: bool) -> Item {
    let s = item_style(cx, b);
    let e = if cx.has_edges(b) {
        cx.edges(s, inner.width)
    } else {
        Edges::default()
    };
    let (outer_main, outer_cross) = if is_row {
        (
            e.m(1) + e.m(3) + e.bp_inline(),
            e.m(0) + e.m(2) + e.bp_block(),
        )
    } else {
        (
            e.m(0) + e.m(2) + e.bp_block(),
            e.m(1) + e.m(3) + e.bp_inline(),
        )
    };
    let mut it = Item {
        b,
        e,
        outer_main,
        outer_cross,
        base: Au::ZERO,
        min: Au::ZERO,
        max: Au::MAX,
        grow: f32::from_bits(s.rare.flex_grow),
        shrink: f32::from_bits(s.rare.flex_shrink),
        main: Au::ZERO,
        frozen: false,
        laid: None,
        cross: Au::ZERO,
        fit: None,
    };
    if fit {
        let (lo, hi) = intrinsic::content_widths(cx, b, inner.width);
        it.fit = Some(hi.min((inner.width - it.outer_cross).max(Au::ZERO)).max(lo));
    }
    it.base = base_size(cx, &it, s, is_row, inner);
    let (bp, min, max, base) = if is_row {
        (
            e.bp_inline(),
            s.boxm.min_width,
            s.boxm.max_width,
            Some(inner.width),
        )
    } else {
        (
            e.bp_block(),
            s.boxm.min_height,
            s.boxm.max_height,
            inner.height,
        )
    };
    it.max = size(max, base).map_or(Au::MAX, |v| content_size(s, v, bp));
    it.min = match size(min, base) {
        Some(v) => content_size(s, v, bp),
        // the automatic minimum: no smaller than its min-content width, unless it scrolls, and no larger than
        // its specified width or its maximum
        None if is_row && s.place.overflow_x == Overflow::Visible => {
            let lo = intrinsic::content_widths(cx, b, inner.width).0;
            size(s.boxm.width, base)
                .map_or(lo, |w| lo.min(content_size(s, w, bp)))
                .min(it.max)
        }
        None => Au::ZERO,
    };
    it
}

// CSS Flexbox 9.7: grow or shrink the line's items to fill `avail`, freezing those their limits stop
pub(super) fn resolve(items: &mut [Item], avail: Au) {
    let hypo: Au = items.iter().fold(Au::ZERO, |a, i| {
        a + i.outer_main + i.base.clamp_lo_hi(i.min, i.max)
    });
    let grow = hypo < avail;
    for it in items.iter_mut() {
        it.frozen = if grow {
            it.grow == 0.0
        } else {
            it.shrink == 0.0
        };
        it.main = it.base.clamp_lo_hi(it.min, it.max);
    }
    for _ in 0..=items.len() {
        let used = items.iter().fold(Au::ZERO, |a, i| {
            a + i.outer_main + if i.frozen { i.main } else { i.base }
        });
        let free = avail - used;
        let active: Vec<usize> = (0..items.len()).filter(|&k| !items[k].frozen).collect();
        if active.is_empty() {
            return;
        }
        let weight: f32 = active
            .iter()
            .map(|&k| {
                let i = &items[k];
                if grow { i.grow } else { i.shrink * i.base.px() }
            })
            .sum();
        let mut violation = Au::ZERO;
        for &k in &active {
            let i = &mut items[k];
            let share = if weight > 0.0 {
                let w = if grow { i.grow } else { i.shrink * i.base.px() };
                free.scale(w / weight.max(if grow { 1.0 } else { 0.0 }))
            } else {
                Au::ZERO
            };
            let target = i.base + share;
            let clamped = target.clamp_lo_hi(i.min, i.max);
            violation += clamped - target;
            i.main = clamped;
        }
        if violation == Au::ZERO {
            return;
        }
        for &k in &active {
            let i = &mut items[k];
            let target_hit = if violation > Au::ZERO {
                i.main == i.min
            } else {
                i.main == i.max
            };
            if target_hit {
                i.frozen = true;
            }
        }
    }
}

pub(super) fn break_into_lines(
    items: &[Item],
    avail: Option<Au>,
    wrap: bool,
    gap: Au,
) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let (mut start, mut used) = (0, Au::ZERO);
    for (k, i) in items.iter().enumerate() {
        let h = i.outer_main + i.base.clamp_lo_hi(i.min, i.max);
        if wrap && k > start && avail.is_some_and(|a| used + h > a) {
            lines.push(start..k);
            (start, used) = (k, Au::ZERO);
        }
        used += h + gap;
    }
    lines.push(start..items.len());
    lines
}

/// The min-content and max-content widths of a flex container's content: its items side by side in a row,
/// the widest of them in a column.
pub fn content_widths(cx: &mut Cx, b: u32, cw: Au) -> (Au, Au) {
    let s = cx.style(b);
    let is_row = super::row(s);
    let gap = s
        .rare
        .column_gap
        .filter(|g| !g.has_pct())
        .map_or(Au::ZERO, |g| Au(g.px));
    let kids: Vec<u32> = cx.tree.children(b).collect();
    let (mut lo, mut hi, mut n) = (Au::ZERO, Au::ZERO, 0);
    for c in kids {
        if cx.has_edges(c) && position::out_of_flow(cx.style(c)) {
            continue;
        }
        let (a, z) = intrinsic::margin_box_widths(cx, c, cw);
        lo = lo.max(a);
        hi = if is_row {
            hi + z + if n > 0 { gap } else { Au::ZERO }
        } else {
            hi.max(z)
        };
        n += 1;
    }
    (lo, hi)
}
