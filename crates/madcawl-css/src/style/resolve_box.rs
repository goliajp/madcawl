//! Resolving the box, positioning and flex properties.

use std::sync::Arc;

use super::computed::{ComputedStyle, Size, VAlign};
use super::length::{Ctx, Lp, to_fixed};
use super::props::{Prop, Specified};
use super::resolve::{Cx, color};

/// A border width snapped to device pixels as WebKit draws it: anything
/// above zero is at least one device pixel, the rest rounds down.
fn snap_border(px: f32, scale: f32) -> i32 {
    if px <= 0.0 {
        return 0;
    }
    let dev = (px * scale).floor().max(1.0);
    to_fixed(dev / scale)
}

pub(crate) fn len(v: &Specified, l: &Ctx, clamp: bool) -> Option<Lp> {
    match v {
        Specified::Len(x) => x.resolve(l, clamp),
        _ => unreachable!("parsed as a length"),
    }
}

fn size(v: &Specified, l: &Ctx) -> Option<Size> {
    Some(match v {
        Specified::Size(k) => Size::Kw(*k),
        Specified::Len(x) => Size::Lp(x.resolve(l, true)?),
        _ => unreachable!("parsed as a size"),
    })
}

pub(crate) fn apply_box(s: &mut ComputedStyle, p: Prop, v: &Specified, l: &Ctx, cx: &Cx) -> bool {
    use Prop as P;
    let i = |base: Prop| p as usize - base as usize;
    match (p, v) {
        (P::MarginTop | P::MarginRight | P::MarginBottom | P::MarginLeft, v) => {
            let Some(x) = margin_like(v, l) else {
                return false;
            };
            Arc::make_mut(&mut s.boxm).margin[i(P::MarginTop)] = x;
        }
        (P::PaddingTop | P::PaddingRight | P::PaddingBottom | P::PaddingLeft, v) => {
            let Some(x) = len(v, l, true) else {
                return false;
            };
            Arc::make_mut(&mut s.boxm).padding[i(P::PaddingTop)] = x;
        }
        (
            P::BorderTopWidth | P::BorderRightWidth | P::BorderBottomWidth | P::BorderLeftWidth,
            v,
        ) => {
            let Some(x) = len(v, l, true) else {
                return false;
            };
            Arc::make_mut(&mut s.boxm).border_width[i(P::BorderTopWidth)] =
                snap_border(x.px_f32(), cx.scale);
        }
        (P::Width | P::Height | P::MinWidth | P::MinHeight | P::MaxWidth | P::MaxHeight, v) => {
            let Some(x) = size(v, l) else { return false };
            let b = Arc::make_mut(&mut s.boxm);
            *[
                &mut b.width,
                &mut b.height,
                &mut b.min_width,
                &mut b.min_height,
                &mut b.max_width,
                &mut b.max_height,
            ][i(P::Width)] = x;
        }
        (P::BoxSizing, Specified::BoxSizing(b)) => Arc::make_mut(&mut s.boxm).box_sizing = *b,
        (_, Specified::BorderStyle(b)) => {
            Arc::make_mut(&mut s.paint).border_style[i(P::BorderTopStyle)] = *b
        }
        (P::BackgroundColor, Specified::Color(c)) => {
            Arc::make_mut(&mut s.paint).background_color = color(c, cx.dark)
        }
        (_, Specified::Color(c)) => {
            Arc::make_mut(&mut s.paint).border_color[i(P::BorderTopColor)] = color(c, cx.dark)
        }
        (P::Top | P::Right | P::Bottom | P::Left, v) => {
            let Some(x) = margin_like(v, l) else {
                return false;
            };
            Arc::make_mut(&mut s.place).inset[i(P::Top)] = x;
        }
        (P::ZIndex, v) => {
            Arc::make_mut(&mut s.place).z_index = match v {
                Specified::Integer(n) => Some(*n),
                _ => None,
            }
        }
        (P::Clear, Specified::Clear(c)) => Arc::make_mut(&mut s.place).clear = *c,
        (P::OverflowX, Specified::Overflow(o)) => Arc::make_mut(&mut s.place).overflow_x = *o,
        (P::OverflowY, Specified::Overflow(o)) => Arc::make_mut(&mut s.place).overflow_y = *o,
        _ => return apply_rare(s, p, v, l),
    }
    true
}

fn margin_like(v: &Specified, l: &Ctx) -> Option<Size> {
    Some(match v {
        Specified::Size(k) => Size::Kw(*k),
        Specified::Len(x) => Size::Lp(x.resolve(l, false)?),
        _ => unreachable!("parsed as a margin"),
    })
}

fn apply_rare(s: &mut ComputedStyle, p: Prop, v: &Specified, l: &Ctx) -> bool {
    use Prop as P;
    let r = Arc::make_mut(&mut s.rare);
    match (p, v) {
        (P::FlexDirection, Specified::FlexDirection(d)) => r.flex_direction = *d,
        (P::FlexWrap, Specified::FlexWrap(w)) => r.flex_wrap = *w,
        (P::FlexGrow, Specified::Number(n)) => r.flex_grow = n.to_bits(),
        (P::FlexShrink, Specified::Number(n)) => r.flex_shrink = n.to_bits(),
        (P::FlexBasis, v) => {
            let Some(x) = size(v, l) else { return false };
            r.flex_basis = x;
        }
        (P::Order, Specified::Integer(n)) => r.order = *n,
        (P::JustifyContent, Specified::Align(a)) => r.justify_content = *a,
        (P::AlignItems, Specified::Align(a)) => r.align_items = *a,
        (P::AlignSelf, Specified::Align(a)) => r.align_self = *a,
        (P::AlignContent, Specified::Align(a)) => r.align_content = *a,
        (P::RowGap | P::ColumnGap, v) => {
            let g = match v {
                Specified::Len(x) => {
                    let Some(x) = x.resolve(l, false) else {
                        return false;
                    };
                    Some(x)
                }
                _ => None,
            };
            if p == P::RowGap {
                r.row_gap = g;
            } else {
                r.column_gap = g;
            }
        }
        (P::VerticalAlign, Specified::VAlign(k)) => r.vertical_align = VAlign::Kw(*k),
        (P::VerticalAlign, Specified::Len(x)) => {
            let Some(x) = x.resolve(l, false) else {
                return false;
            };
            r.vertical_align = VAlign::Lp(x);
        }
        _ => unreachable!("{p:?} parsed as {v:?}"),
    }
    true
}
