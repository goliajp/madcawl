//! A box's own paint: its background color and its border, from its computed values.

use madcawl_css::style::{BorderStyle, ComputedStyle, CssColor};

use crate::list::{Item, Line, RectF, Rgba};

/// A computed color as used: `currentcolor` is the text color. A color outside legacy sRGB is not
/// converted yet and paints nothing.
pub fn color(c: &CssColor, s: &ComputedStyle) -> Option<Rgba> {
    match c {
        CssColor::Rgba(v) => Some(Rgba(*v)),
        CssColor::Current => text_color(s),
        CssColor::Other(_) => None,
    }
}

/// The text color of a style; `currentcolor` as the value of `color` itself is black.
pub fn text_color(s: &ComputedStyle) -> Option<Rgba> {
    match &s.text.color {
        CssColor::Rgba(v) => Some(Rgba(*v)),
        CssColor::Current => Some(Rgba::BLACK),
        CssColor::Other(_) => None,
    }
}

pub fn background(s: &ComputedStyle) -> Option<Rgba> {
    color(&s.paint.background_color, s).filter(|c| !c.is_clear())
}

fn line(b: BorderStyle) -> Option<Line> {
    Some(match b {
        BorderStyle::None | BorderStyle::Hidden => return None,
        BorderStyle::Solid => Line::Solid,
        BorderStyle::Dashed => Line::Dashed,
        BorderStyle::Dotted => Line::Dotted,
        BorderStyle::Double => Line::Double,
        BorderStyle::Groove => Line::Groove,
        BorderStyle::Ridge => Line::Ridge,
        BorderStyle::Inset => Line::Inset,
        BorderStyle::Outset => Line::Outset,
    })
}

/// The border item of a box with border box `rect`, faded by `alpha`; `None` when no side shows.
pub fn border(s: &ComputedStyle, rect: RectF, alpha: f32) -> Option<Item> {
    let p = &s.paint;
    let mut widths = [0.0f32; 4];
    let mut colors = [Rgba([0; 4]); 4];
    let mut lines = [Line::Solid; 4];
    let mut any = false;
    for k in 0..4 {
        let w = s.boxm.border_width[k] as f32 / 64.0;
        let (Some(l), Some(c)) = (line(p.border_style[k]), color(&p.border_color[k], s)) else {
            continue;
        };
        if w <= 0.0 || c.is_clear() {
            continue;
        }
        (widths[k], colors[k], lines[k]) = (w, c.fade(alpha), l);
        any = true;
    }
    any.then_some(Item::Border {
        rect,
        widths,
        colors,
        lines,
    })
}

/// Whether any side of the box has a border width.
pub fn bordered(s: &ComputedStyle) -> bool {
    s.boxm.border_width.iter().any(|&w| w > 0)
}

/// A border of one piece of an inline box split across lines, as `box-decoration-break: slice` draws it:
/// the left side only on the first piece, the right only on the last. Layout lays the pieces out left to
/// right and gives the first one the left margin, border and padding in either direction, so the sides
/// here follow it.
pub fn slice(b: &mut Item, first: bool, last: bool) {
    if let Item::Border { widths, .. } = b {
        if !first {
            widths[3] = 0.0;
        }
        if !last {
            widths[1] = 0.0;
        }
    }
}
