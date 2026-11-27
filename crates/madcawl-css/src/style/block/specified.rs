//! Declared values written out as CSSOM serializes a specified value:
//! keywords in lowercase, numbers in their shortest form, lengths in the
//! unit they were declared in where the declared value kept it (absolute
//! units other than px were turned into px when parsed), and colors as
//! `rgb()` or `rgba()`.

use std::fmt::Write;

use super::super::cssom::number;
use super::super::fonts::SIZE_KEYWORDS;
use super::super::grid::{write_areas, write_entries, write_flow, write_line, write_size};
use super::super::length::{Expr, Form, Linear, SpecLen};
use super::super::props::{FontSize, Prop, SpecColor, Specified, Weight, Wide};

fn wide(w: Wide) -> &'static str {
    match w {
        Wide::Initial => "initial",
        Wide::Inherit => "inherit",
        Wide::Unset => "unset",
        Wide::Revert => "revert",
        Wide::RevertLayer => "revert-layer",
    }
}

fn f(v: f32, out: &mut String) {
    number(f64::from(v), out);
}

/// The units of a `Linear`, in the order CSS Values sorts the terms of
/// a sum: the percentage, then the dimensions by unit, alphabetically.
fn terms(l: &Linear) -> [(f32, &'static str); 10] {
    [
        (l.pct, "%"),
        (l.ch, "ch"),
        (l.em, "em"),
        (l.ex, "ex"),
        (l.px, "px"),
        (l.rem, "rem"),
        (l.vh, "vh"),
        (l.vmax, "vmax"),
        (l.vmin, "vmin"),
        (l.vw, "vw"),
    ]
}

/// A sum of terms, as the inside of `calc()` writes it.
fn sum(l: &Linear, out: &mut String) {
    let mut first = true;
    for (v, u) in terms(l).into_iter().filter(|t| t.0 != 0.0) {
        if first {
            f(v, out);
        } else {
            out.push_str(if v < 0.0 { " - " } else { " + " });
            f(v.abs(), out);
        }
        out.push_str(u);
        first = false;
    }
    if first {
        out.push_str("0px");
    }
}

fn expr(e: &Expr, out: &mut String) {
    match e {
        Expr::Leaf(l) => sum(l, out),
        Expr::Sum(a, b) => {
            expr(a, out);
            out.push_str(" + ");
            expr(b, out);
        }
        Expr::Scale(a, k) => {
            out.push('(');
            expr(a, out);
            out.push_str(") * ");
            f(*k, out);
        }
        Expr::Min(v) | Expr::Max(v) => {
            out.push_str(if matches!(e, Expr::Min(_)) {
                "min("
            } else {
                "max("
            });
            for (k, x) in v.iter().enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                expr(x, out);
            }
            out.push(')');
        }
    }
}

/// A declared `<length-percentage>`.
pub(crate) fn write_len(v: &SpecLen, out: &mut String) {
    match v {
        SpecLen::Lin { v, form } => {
            let mut nonzero = terms(v).into_iter().filter(|t| t.0 != 0.0);
            match (form, nonzero.next(), nonzero.next()) {
                (Form::Pct, None, _) => out.push_str("0%"),
                (Form::Len, None, _) => out.push_str("0px"),
                (Form::Len | Form::Pct, Some((x, u)), None) => {
                    f(x, out);
                    out.push_str(u);
                }
                _ => {
                    out.push_str("calc(");
                    sum(v, out);
                    out.push(')');
                }
            }
        }
        SpecLen::Expr(e) => match &**e {
            Expr::Min(_) | Expr::Max(_) => expr(e, out),
            e => {
                out.push_str("calc(");
                expr(e, out);
                out.push(')');
            }
        },
    }
}

fn rgba(c: [u8; 4], out: &mut String) {
    let [r, g, b, a] = c;
    if a == 255 {
        let _ = write!(out, "rgb({r}, {g}, {b})");
    } else {
        let _ = write!(out, "rgba({r}, {g}, {b}, ");
        // the alpha byte as the shortest of two decimals that rounds to it
        number((f64::from(a) / 255.0 * 100.0).round() / 100.0, out);
        out.push(')');
    }
}

pub(crate) fn write_color(c: &SpecColor, out: &mut String) {
    match c {
        SpecColor::Current => out.push_str("currentcolor"),
        SpecColor::Rgba(v) => rgba(*v, out),
        SpecColor::LightDark(l, d) => {
            out.push_str("light-dark(");
            rgba(*l, out);
            out.push_str(", ");
            rgba(*d, out);
            out.push(')');
        }
        SpecColor::Other(s) => out.push_str(s),
    }
}

/// The keyword a keyword value is.
fn keyword(v: &Specified) -> Option<&'static str> {
    use Specified as S;
    Some(match v {
        S::Display(k) => k.css(),
        S::Visibility(k) => k.css(),
        S::ContentVisibility(k) => k.css(),
        S::Position(k) => k.css(),
        S::Float(k) => k.css(),
        S::BoxSizing(k) => k.css(),
        S::BorderStyle(k) => k.css(),
        S::Overflow(k) => k.css(),
        S::Clear(k) => k.css(),
        S::WhiteSpaceCollapse(k) => k.css(),
        S::TextWrapMode(k) => k.css(),
        S::TextAlign(k) => k.css(),
        S::Direction(k) => k.css(),
        S::WritingMode(k) => k.css(),
        S::ListStyleType(k) => k.css(),
        S::ListStylePosition(k) => k.css(),
        S::FontStyle(k) => k.css(),
        S::FlexDirection(k) => k.css(),
        S::FlexWrap(k) => k.css(),
        S::Align(k) => k.css(),
        S::VAlign(k) => k.css(),
        S::TableLayout(k) => k.css(),
        S::BorderCollapse(k) => k.css(),
        S::CaptionSide(k) => k.css(),
        S::Size(k) => k.css(),
        S::Wide(w) => wide(*w),
        S::FontSize(FontSize::Keyword(k)) => SIZE_KEYWORDS[usize::from(*k)],
        S::FontSize(FontSize::Smaller) => "smaller",
        S::FontSize(FontSize::Larger) => "larger",
        S::Weight(Weight::Bolder) => "bolder",
        S::Weight(Weight::Lighter) => "lighter",
        _ => return None,
    })
}

/// `v`, the declared value of `p`, serialized; nothing for a value that
/// holds `var()`, whose text the declaration keeps.
pub(crate) fn write_specified(p: Prop, v: &Specified, out: &mut String) {
    use Specified as S;
    let len = |l: &SpecLen, o: &mut String| write_len(l, o);
    if let Some(k) = keyword(v) {
        return out.push_str(k);
    }
    match v {
        S::Len(l) => write_len(l, out),
        S::Number(n) => f(*n, out),
        S::Integer(n) => {
            let _ = write!(out, "{n}");
        }
        // the `normal` of the gaps is kept as `Auto`
        S::Auto if matches!(p, Prop::RowGap | Prop::ColumnGap) => out.push_str("normal"),
        S::Auto => out.push_str("auto"),
        S::Normal => out.push_str("normal"),
        S::Color(c) => write_color(c, out),
        S::Family(s) => out.push_str(s),
        S::FontSize(FontSize::Len(l)) => write_len(l, out),
        S::Weight(Weight::Abs(w)) => {
            let _ = write!(out, "{w}");
        }
        S::Content(c) => super::super::generated::serialize_content(c, out),
        S::AspectRatio(a) => super::super::generated::serialize_aspect_ratio(*a, out),
        S::Tracks(None) => out.push_str("none"),
        S::Tracks(Some(t)) => write_entries(&t.0, out, &len),
        S::AutoTracks(v) => {
            for (k, s) in v.iter().enumerate() {
                if k > 0 {
                    out.push(' ');
                }
                write_size(s, out, &len);
            }
        }
        S::Areas(a) => write_areas(a, out),
        S::GridLine(g) => write_line(g, out),
        S::AutoFlow(a) => write_flow(*a, true, out),
        S::Spacing(a, b) => {
            write_len(a, out);
            if a != b {
                out.push(' ');
                write_len(b, out);
            }
        }
        S::Unresolved => {}
        _ => unreachable!("a keyword, written above"),
    }
}
