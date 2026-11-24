//! From a declared value to the computed one: lengths to 1/64 px
//! against the font size and the viewport, font sizes and weights
//! against the parent's, and the fix-ups CSS makes once every value is
//! known.

use std::sync::Arc;

use super::computed::{ComputedStyle, CssColor, LineHeight};
use super::kw::*;
use super::length::{Ctx, to_fixed};
use super::props::{FontSize, Prop, SpecColor, Specified, Weight};
use super::resolve_box::{apply_box, len};

/// What resolving needs beyond the parent's values.
#[derive(Clone, Copy)]
pub(crate) struct Cx {
    pub root_font: f32,
    pub vw: f32,
    pub vh: f32,
    /// Device pixels per CSS pixel
    pub scale: f32,
    pub dark: bool,
}

impl Cx {
    fn len(&self, em: f32) -> Ctx {
        Ctx {
            em,
            rem: self.root_font,
            vw: self.vw,
            vh: self.vh,
        }
    }
}

/// The sizes of `xx-small` to `xxx-large`, in WebKit's table for a
/// 16px medium; for `monospace`, whose medium is 13px, the same steps
/// scaled.
fn keyword_size(k: u8, monospace: bool) -> f32 {
    const PROPORTIONAL: [f32; 8] = [9.0, 10.0, 13.0, 16.0, 18.0, 24.0, 32.0, 48.0];
    const MONOSPACE: [f32; 8] = [9.0, 10.0, 11.0, 13.0, 15.0, 20.0, 26.0, 39.0];
    let t = if monospace { &MONOSPACE } else { &PROPORTIONAL };
    t[usize::from(k.min(7))]
}

fn is_monospace(s: &ComputedStyle) -> bool {
    &*s.font.family == "monospace"
}

pub(crate) fn color(c: &SpecColor, dark: bool) -> CssColor {
    match c {
        SpecColor::Current => CssColor::Current,
        SpecColor::Rgba(v) => CssColor::Rgba(*v),
        SpecColor::LightDark(l, d) => CssColor::Rgba(if dark { *d } else { *l }),
        SpecColor::Other(s) => CssColor::Other(s.clone()),
    }
}

fn bolder(w: u16) -> u16 {
    match w {
        0..350 => 400,
        350..550 => 700,
        _ => 900,
    }
}

fn lighter(w: u16) -> u16 {
    match w {
        0..100 => w,
        100..550 => 100,
        550..750 => 400,
        _ => 700,
    }
}

/// Set `p` on `s` from its declared value `v`; `false` when it turns
/// out invalid now that the font size is known, which makes it unset.
pub(crate) fn apply(
    s: &mut ComputedStyle,
    p: Prop,
    v: &Specified,
    parent: &ComputedStyle,
    cx: &Cx,
) -> bool {
    use Prop as P;
    let l = cx.len(s.font.size());
    match (p, v) {
        (P::Display, Specified::Display(d)) => s.display = *d,
        (P::Visibility, Specified::Visibility(d)) => s.visibility = *d,
        (P::ContentVisibility, Specified::ContentVisibility(d)) => s.content_visibility = *d,
        (P::Opacity, Specified::Number(o)) => s.set_opacity(*o),
        (P::Position, Specified::Position(d)) => s.position = *d,
        (P::Float, Specified::Float(d)) => s.float = *d,
        (P::FontFamily, Specified::Family(f)) => Arc::make_mut(&mut s.font).family = f.clone(),
        (P::FontSize, Specified::FontSize(f)) => return font_size(s, f, parent, cx),
        (P::FontWeight, Specified::Weight(w)) => {
            let pw = parent.font.weight;
            Arc::make_mut(&mut s.font).weight = match w {
                Weight::Abs(w) => *w,
                Weight::Bolder => bolder(pw),
                Weight::Lighter => lighter(pw),
            }
        }
        (P::FontStyle, Specified::FontStyle(f)) => Arc::make_mut(&mut s.font).style = *f,
        (P::LineHeight, v) => {
            let lh = match v {
                Specified::Normal => LineHeight::Normal,
                Specified::Number(n) => LineHeight::Number(n.to_bits()),
                Specified::Len(x) => {
                    let Some(r) = x.resolve(&l, true) else {
                        return false;
                    };
                    LineHeight::Px(r.px.saturating_add(to_fixed(r.pct() * s.font.size() / 100.0)))
                }
                _ => unreachable!("parsed as a line height"),
            };
            Arc::make_mut(&mut s.font).line_height = lh;
        }
        (P::Color, Specified::Color(c)) => {
            Arc::make_mut(&mut s.text).color = match color(c, cx.dark) {
                CssColor::Current => parent.text.color.clone(),
                c => c,
            }
        }
        (P::WhiteSpaceCollapse, Specified::WhiteSpaceCollapse(w)) => {
            Arc::make_mut(&mut s.text).white_space_collapse = *w
        }
        (P::TextWrapMode, Specified::TextWrapMode(w)) => {
            Arc::make_mut(&mut s.text).text_wrap_mode = *w
        }
        (P::TextAlign, Specified::TextAlign(t)) => {
            let t = match t {
                TextAlign::MatchParent => match (parent.text.text_align, parent.text.direction) {
                    (TextAlign::Start, Direction::Ltr) | (TextAlign::End, Direction::Rtl) => {
                        TextAlign::Left
                    }
                    (TextAlign::Start | TextAlign::End, _) => TextAlign::Right,
                    (t, _) => t,
                },
                t => *t,
            };
            Arc::make_mut(&mut s.text).text_align = t;
        }
        (P::Direction, Specified::Direction(d)) => Arc::make_mut(&mut s.text).direction = *d,
        (P::WritingMode, Specified::WritingMode(w)) => Arc::make_mut(&mut s.text).writing_mode = *w,
        (P::ListStyleType, Specified::ListStyleType(t)) => {
            Arc::make_mut(&mut s.text).list_style_type = *t
        }
        (P::ListStylePosition, Specified::ListStylePosition(t)) => {
            Arc::make_mut(&mut s.text).list_style_position = *t
        }
        (P::TextIndent, v) => {
            let Some(x) = len(v, &l, false) else {
                return false;
            };
            Arc::make_mut(&mut s.text).text_indent = x;
        }
        _ => return apply_box(s, p, v, &l, cx),
    }
    true
}

fn font_size(s: &mut ComputedStyle, f: &FontSize, parent: &ComputedStyle, cx: &Cx) -> bool {
    let ps = parent.font.size();
    let mono = is_monospace(s);
    let (size, kw) = match f {
        FontSize::Keyword(k) => (keyword_size(*k, mono), Some(*k)),
        FontSize::Smaller | FontSize::Larger => {
            let up = matches!(f, FontSize::Larger);
            match parent.font.size_keyword {
                Some(k) if up && k < 7 || !up && k > 0 => {
                    let k = if up { k + 1 } else { k - 1 };
                    (keyword_size(k, mono), Some(k))
                }
                _ => (if up { ps * 1.2 } else { ps / 1.2 }, None),
            }
        }
        FontSize::Len(x) => {
            // em and percentages are of the parent's size
            let Some((px, pct)) = x.resolve_float(&cx.len(ps)) else {
                return false;
            };
            ((px + pct * ps / 100.0).max(0.0), None)
        }
    };
    let font = Arc::make_mut(&mut s.font);
    font.set_size(size);
    font.size_keyword = kw;
    true
}

/// After the font family and size: a size from a keyword follows the
/// family to or from `monospace`.
pub(crate) fn fix_font(s: &mut ComputedStyle) {
    if let Some(k) = s.font.size_keyword {
        let want = keyword_size(k, is_monospace(s));
        if s.font.size() != want {
            Arc::make_mut(&mut s.font).set_size(want);
        }
    }
}

/// Once every value is known: a border of style `none` or `hidden` has
/// no width, and `overflow` that is visible on one axis only is not.
pub(crate) fn fix_up(s: &mut ComputedStyle) {
    for i in 0..4 {
        let none = matches!(
            s.paint.border_style[i],
            BorderStyle::None | BorderStyle::Hidden
        );
        if none && s.boxm.border_width[i] != 0 {
            Arc::make_mut(&mut s.boxm).border_width[i] = 0;
        }
    }
    let (x, y) = (s.place.overflow_x, s.place.overflow_y);
    let scrolls = |o| !matches!(o, Overflow::Visible | Overflow::Clip);
    if scrolls(x) != scrolls(y) {
        let fix = |o| match o {
            Overflow::Visible => Overflow::Auto,
            Overflow::Clip => Overflow::Hidden,
            o => o,
        };
        let pl = Arc::make_mut(&mut s.place);
        pl.overflow_x = fix(x);
        pl.overflow_y = fix(y);
    }
}
