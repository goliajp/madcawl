//! Computed values written out as `getComputedStyle` writes them, for
//! the properties whose resolved value is the computed value; the box
//! sizes and offsets, which it gives as used values, are written as
//! computed.

use std::fmt::Write;

use super::computed::{ComputedStyle, CssColor, LineHeight, Size, VAlign};
use super::length::{Form, Lp, UNIT};
use super::props::Prop;

/// A number the way WebKit writes a `double`: at most six decimals,
/// no trailing zeros.
pub fn number(v: f64, out: &mut String) {
    let mut s = format!("{:.6}", v);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" {
        s = "0".into();
    }
    out.push_str(&s);
}

fn px(v: impl Into<f64>, out: &mut String) {
    number(v.into() / f64::from(UNIT), out);
    out.push_str("px");
}

fn lp(v: &Lp, out: &mut String) {
    match v.form {
        Form::Len => px(v.px, out),
        Form::Pct => {
            number(f64::from(v.pct()), out);
            out.push('%');
        }
        Form::Calc => {
            out.push_str("calc(");
            number(f64::from(v.pct()), out);
            out.push_str(if v.px < 0 { "% - " } else { "% + " });
            px(v.px.unsigned_abs(), out);
            out.push(')');
        }
    }
}

fn size(v: &Size, out: &mut String) {
    match v {
        Size::Kw(k) => out.push_str(k.css()),
        Size::Lp(l) => lp(l, out),
    }
}

fn color(c: &CssColor, current: &CssColor, out: &mut String) {
    match c {
        CssColor::Current => color(current, &CssColor::Rgba([0, 0, 0, 255]), out),
        CssColor::Other(s) => out.push_str(s),
        CssColor::Rgba([r, g, b, a]) => {
            if *a == 255 {
                let _ = write!(out, "rgb({r}, {g}, {b})");
            } else {
                let _ = write!(out, "rgba({r}, {g}, {b}, ");
                // the alpha byte back to the shortest of two decimals
                // that rounds to it
                let a2 = (f64::from(*a) / 255.0 * 100.0).round() / 100.0;
                number(a2, out);
                out.push(')');
            }
        }
    }
}

impl ComputedStyle {
    /// `p`'s computed value as `getComputedStyle` writes it.
    pub fn css(&self, p: Prop) -> String {
        let mut o = String::new();
        self.write_css(p, &mut o);
        o
    }

    pub fn write_css(&self, p: Prop, o: &mut String) {
        use Prop as P;
        let (f, t, b, pa, pl, r) = (
            &self.font,
            &self.text,
            &self.boxm,
            &self.paint,
            &self.place,
            &self.rare,
        );
        let side = |base: Prop| p as usize - base as usize;
        let n = |v: u32, o: &mut String| number(f64::from(f32::from_bits(v)), o);
        match p {
            P::Display => o.push_str(self.display.css()),
            P::Visibility => o.push_str(self.visibility.css()),
            P::ContentVisibility => o.push_str(self.content_visibility.css()),
            P::Opacity => number(f64::from(self.opacity()), o),
            P::Position => o.push_str(self.position.css()),
            P::Float => o.push_str(self.float.css()),
            P::FontFamily => o.push_str(&f.family),
            P::FontSize => {
                number(f64::from(f.size()), o);
                o.push_str("px");
            }
            P::FontWeight => {
                let _ = write!(o, "{}", f.weight);
            }
            P::FontStyle => o.push_str(f.style.css()),
            P::LineHeight => match f.line_height {
                LineHeight::Normal => o.push_str("normal"),
                LineHeight::Number(k) => {
                    number(f64::from(f32::from_bits(k) * f.size()), o);
                    o.push_str("px");
                }
                LineHeight::Px(v) => px(v, o),
            },
            P::Color => color(&t.color, &t.color, o),
            P::WhiteSpaceCollapse => o.push_str(t.white_space_collapse.css()),
            P::TextWrapMode => o.push_str(t.text_wrap_mode.css()),
            P::TextAlign => o.push_str(t.text_align.css()),
            P::Direction => o.push_str(t.direction.css()),
            P::WritingMode => o.push_str(t.writing_mode.css()),
            P::ListStyleType => o.push_str(t.list_style_type.css()),
            P::ListStylePosition => o.push_str(t.list_style_position.css()),
            P::TextIndent => lp(&t.text_indent, o),
            P::MarginTop | P::MarginRight | P::MarginBottom | P::MarginLeft => {
                size(&b.margin[side(P::MarginTop)], o)
            }
            P::PaddingTop | P::PaddingRight | P::PaddingBottom | P::PaddingLeft => {
                lp(&b.padding[side(P::PaddingTop)], o)
            }
            P::BorderTopWidth | P::BorderRightWidth | P::BorderBottomWidth | P::BorderLeftWidth => {
                px(b.border_width[side(P::BorderTopWidth)], o)
            }
            P::Width => size(&b.width, o),
            P::Height => size(&b.height, o),
            P::MinWidth => size(&b.min_width, o),
            P::MinHeight => size(&b.min_height, o),
            P::MaxWidth => size(&b.max_width, o),
            P::MaxHeight => size(&b.max_height, o),
            P::BoxSizing => o.push_str(b.box_sizing.css()),
            P::BorderTopStyle | P::BorderRightStyle | P::BorderBottomStyle | P::BorderLeftStyle => {
                o.push_str(pa.border_style[side(P::BorderTopStyle)].css())
            }
            P::BorderTopColor | P::BorderRightColor | P::BorderBottomColor | P::BorderLeftColor => {
                color(&pa.border_color[side(P::BorderTopColor)], &t.color, o)
            }
            P::BackgroundColor => color(&pa.background_color, &t.color, o),
            P::Top | P::Right | P::Bottom | P::Left => size(&pl.inset[side(P::Top)], o),
            P::ZIndex => match pl.z_index {
                Some(z) => {
                    let _ = write!(o, "{z}");
                }
                None => o.push_str("auto"),
            },
            P::Clear => o.push_str(pl.clear.css()),
            P::OverflowX => o.push_str(pl.overflow_x.css()),
            P::OverflowY => o.push_str(pl.overflow_y.css()),
            P::FlexDirection => o.push_str(r.flex_direction.css()),
            P::FlexWrap => o.push_str(r.flex_wrap.css()),
            P::FlexGrow => n(r.flex_grow, o),
            P::FlexShrink => n(r.flex_shrink, o),
            P::FlexBasis => size(&r.flex_basis, o),
            P::Order => {
                let _ = write!(o, "{}", r.order);
            }
            P::JustifyContent => o.push_str(r.justify_content.css()),
            P::AlignItems => o.push_str(r.align_items.css()),
            P::AlignSelf => o.push_str(r.align_self.css()),
            P::AlignContent => o.push_str(r.align_content.css()),
            P::RowGap | P::ColumnGap => {
                match if p == P::RowGap {
                    &r.row_gap
                } else {
                    &r.column_gap
                } {
                    Some(l) => lp(l, o),
                    None => o.push_str("normal"),
                }
            }
            P::VerticalAlign => match &r.vertical_align {
                VAlign::Kw(k) => o.push_str(k.css()),
                VAlign::Lp(l) => lp(l, o),
            },
        }
    }
}
