//! Flex layout (CSS Flexbox 9): items in `order`, their flex base sizes, lines when the container wraps,
//! flexible lengths resolved per line, then main-axis and cross-axis alignment. Items are laid out by block
//! layout with the size flexing gave them fixed.

use madcawl_css::style::{Align, ComputedStyle, Display, FlexDirection, FlexWrap, SizeKw};

use crate::au::{Au, Point};
use crate::cx::{Cx, Space};
use crate::fragment::Child;
use crate::position;

mod cross;
mod item;

use cross::{cross_sizes, measures};
use item::{Item, break_into_lines, item, item_style, resolve};
pub use item::{content_widths, is_item};

/// What a flex container's content came to.
pub struct Flexed {
    pub children: Vec<Child>,
    /// Out-of-flow boxes below it, from the content box corner
    pub statics: Vec<(u32, Point)>,
    pub height: Au,
    pub baseline: Option<Au>,
}

pub fn is_flex(s: &ComputedStyle) -> bool {
    matches!(
        s.display,
        Display::Flex | Display::InlineFlex | Display::WebkitBox | Display::WebkitInlineBox
    )
}

fn row(s: &ComputedStyle) -> bool {
    matches!(
        s.rare.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    )
}

fn align_self(s: &ComputedStyle, parent: &ComputedStyle) -> Align {
    match s.rare.align_self {
        Align::Auto => parent.rare.align_items,
        a => a,
    }
}

/// Lays out the items of flex container `b` in its content box `inner`, whose height has the minimum and
/// maximum `limits` when it is not definite.
pub fn layout(cx: &mut Cx, b: u32, inner: Space, limits: (Au, Au)) -> Flexed {
    let s = cx.style(b);
    let is_row = row(s);
    let mut statics = Vec::new();
    let mut kids = Vec::new();
    for c in cx.tree.children(b) {
        // an out-of-flow child's static position is the content box's corner
        if cx.has_edges(c) && position::out_of_flow(cx.style(c)) {
            statics.push((c, Point::default()));
        } else {
            kids.push(c);
        }
    }
    kids.sort_by_key(|&c| item_style(cx, c).rare.order);
    let mut items: Vec<Item> = kids
        .iter()
        .map(|&c| {
            // across a column, an item that does not stretch fits its content
            let cs = item_style(cx, c);
            let auto = |v| matches!(v, madcawl_css::style::Size::Kw(SizeKw::Auto));
            let aligned = matches!(align_self(cs, s), Align::Stretch | Align::Normal);
            let fit = !is_row && !aligned && auto(cs.boxm.width);
            let stretch = aligned
                && auto(if is_row {
                    cs.boxm.height
                } else {
                    cs.boxm.width
                });
            item(cx, c, is_row, inner, (fit, stretch))
        })
        .collect();
    let main_avail = if is_row {
        Some(inner.width)
    } else {
        inner.height
    };
    let gap = |g: Option<madcawl_css::style::Lp>, base: Option<Au>| {
        g.map_or(Au::ZERO, |l| {
            if l.has_pct() {
                base.map_or(Au(l.px), |b| crate::style::lp(l, b))
            } else {
                Au(l.px)
            }
        })
    };
    let (main_gap, cross_gap) = if is_row {
        (
            gap(s.rare.column_gap, Some(inner.width)),
            gap(s.rare.row_gap, inner.height),
        )
    } else {
        (
            gap(s.rare.row_gap, inner.height),
            gap(s.rare.column_gap, Some(inner.width)),
        )
    };
    let wrap = s.rare.flex_wrap != FlexWrap::Nowrap;
    let lines = break_into_lines(&items, main_avail, wrap, main_gap);
    let mut out = Vec::new();
    let mut cross_at = Au::ZERO;
    let mut baseline = None;
    let single = lines.len() == 1 && !wrap;
    for (n, line) in lines.iter().cloned().enumerate() {
        let gaps = Au(main_gap
            .0
            .saturating_mul(line.len().saturating_sub(1) as i32));
        let avail = main_avail.unwrap_or_else(|| {
            items[line.clone()].iter().fold(gaps, |a, i| {
                a + i.outer_main + i.base.clamp_lo_hi(i.min, i.max)
            })
        });
        resolve(&mut items[line.clone()], avail - gaps);
        if n > 0 {
            cross_at += cross_gap;
        }
        let fixed_cross = single
            .then_some(if is_row {
                inner.height
            } else {
                Some(inner.width)
            })
            .flatten();
        // a single line is as tall as the container's limits let it be
        let cross = (fixed_cross, (single && is_row).then_some(limits));
        let line_cross = cross_sizes(cx, &mut items[line.clone()], is_row, inner, cross);
        let at = (avail, main_gap, cross_at, line_cross);
        let placed = (&mut out, &mut statics, &mut baseline);
        place_line(cx, b, &mut items[line], is_row, at, placed);
        cross_at += line_cross;
    }
    let height = if is_row {
        cross_at
    } else {
        let gaps = Au(main_gap
            .0
            .saturating_mul(items.len().saturating_sub(1) as i32));
        items
            .iter()
            .fold(gaps, |a, i| a + i.outer_main + i.main)
            .max(Au::ZERO)
    };
    Flexed {
        children: out,
        statics,
        height,
        baseline,
    }
}

// `at`: the main size available, the gap between items, and where the line is and how big across
fn place_line(
    cx: &Cx,
    b: u32,
    items: &mut [Item],
    is_row: bool,
    at: (Au, Au, Au, Au),
    placed: (&mut Vec<Child>, &mut Vec<(u32, Point)>, &mut Option<Au>),
) {
    let (avail, main_gap, cross_at, line_cross) = at;
    let (out, statics, baseline) = placed;
    let parent = cx.style(b);
    let gaps = Au(main_gap
        .0
        .saturating_mul(items.len().saturating_sub(1) as i32));
    let used = items.iter().fold(gaps, |a, i| a + i.outer_main + i.main);
    let free = (avail - used).max(Au::ZERO);
    let n = items.len() as i32;
    // auto margins on the main axis take the free space before justify-content does
    let sides = if is_row { (3, 1) } else { (0, 2) };
    let autos: i32 = items
        .iter()
        .map(|i| {
            i32::from(i.e.margin[sides.0].is_none()) + i32::from(i.e.margin[sides.1].is_none())
        })
        .sum();
    let each = if autos > 0 {
        Au(free.0 / autos)
    } else {
        Au::ZERO
    };
    let free = if autos > 0 { Au::ZERO } else { free };
    let (mut at, gap) = match parent.rare.justify_content {
        Align::End | Align::FlexEnd | Align::Right => (free, Au::ZERO),
        Align::Center => (free.half(), Au::ZERO),
        Align::SpaceBetween if n > 1 => (Au::ZERO, Au(free.0 / (n - 1))),
        Align::SpaceAround if n > 0 => (Au(free.0 / (2 * n)), Au(free.0 / n)),
        Align::SpaceEvenly if n > 0 => (Au(free.0 / (n + 1)), Au(free.0 / (n + 1))),
        _ => (Au::ZERO, Au::ZERO),
    };
    let reverse = matches!(
        parent.rare.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    for it in items.iter() {
        let s = item_style(cx, it.b);
        let r = it.laid.as_ref().expect("laid out in cross_sizes");
        let (m_before, m_after, c_before, c_after) = if is_row {
            (it.e.m(3), it.e.m(1), it.e.m(0), it.e.m(2))
        } else {
            (it.e.m(0), it.e.m(2), it.e.m(3), it.e.m(1))
        };
        let auto = |side: usize| {
            if it.e.margin[side].is_none() {
                each
            } else {
                Au::ZERO
            }
        };
        let (m_before, m_after) = (m_before + auto(sides.0), m_after + auto(sides.1));
        let outer = it.cross + c_before + c_after;
        let c = match align_self(s, parent) {
            Align::End | Align::FlexEnd | Align::SelfEnd => line_cross - outer,
            Align::Center => (line_cross - outer).half(),
            _ => Au::ZERO,
        } + c_before
            + cross_at;
        let main_size = if is_row { r.frag.size.w } else { r.frag.size.h };
        let m = if reverse {
            avail - at - m_before - main_size
        } else {
            at + m_before
        };
        let offset = if is_row {
            Point { x: m, y: c }
        } else {
            Point { x: c, y: m }
        };
        // the first item's baseline, or one synthesized from its border box bottom
        if baseline.is_none() {
            *baseline = Some(offset.y + r.baseline.unwrap_or(r.frag.size.h));
        }
        let rel = position::relative_offset(s, Space::new(avail, None), true);
        let pos = offset + rel;
        statics.extend(r.statics.iter().map(|&(a, p)| (a, p + pos)));
        out.push(Child {
            offset: pos,
            frag: r.frag.clone(),
        });
        at += m_before + main_size + m_after + gap + main_gap;
    }
}
