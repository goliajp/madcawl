//! What layout reads of the computed values: lengths resolved against their bases, a box's margins, borders
//! and padding, and the font of a style with the metrics lines are built from.

use std::collections::HashMap;

use madcawl_css::style::{
    BoxSizing, ComputedStyle, Display, Float, Font, FontStyle, LineHeight, Lp, Overflow, Position,
    Size as CssSize,
};
use madcawl_text::{FaceId, Fonts};

use crate::au::Au;

/// A length-percentage against `base`, truncated to the unit as computed lengths are.
pub fn lp(v: Lp, base: Au) -> Au {
    if !v.has_pct() {
        return Au(v.px);
    }
    Au(v.px) + Au(madcawl_css::style::to_fixed(v.pct() / 100.0 * base.px()))
}

/// A size against `base`; `None` for `auto` and the other keywords, and for a percentage of an indefinite base.
pub fn size(s: CssSize, base: Option<Au>) -> Option<Au> {
    match s {
        CssSize::Lp(l) if !l.has_pct() => Some(Au(l.px)),
        CssSize::Lp(l) => base.map(|b| lp(l, b)),
        CssSize::Kw(_) => None,
    }
}

/// Top, right, bottom, left.
pub type Sides = [Au; 4];

/// A box's margins (`None` is `auto`), borders and padding, resolved against the containing block's width.
/// The default is none at all, as an anonymous box has.
#[derive(Clone, Copy, Debug)]
pub struct Edges {
    pub margin: [Option<Au>; 4],
    pub border: Sides,
    pub padding: Sides,
}

impl Default for Edges {
    fn default() -> Edges {
        Edges {
            margin: [Some(Au::ZERO); 4],
            border: [Au::ZERO; 4],
            padding: [Au::ZERO; 4],
        }
    }
}

impl Edges {
    pub fn of(s: &ComputedStyle, cw: Au) -> Edges {
        let b = &s.boxm;
        let margin = b.margin.map(|m| match m {
            CssSize::Lp(l) => Some(lp(l, cw)),
            CssSize::Kw(_) => None,
        });
        Edges {
            margin,
            border: b.border_width.map(Au),
            padding: b.padding.map(|p| lp(p, cw).max(Au::ZERO)),
        }
    }

    pub fn m(&self, side: usize) -> Au {
        self.margin[side].unwrap_or(Au::ZERO)
    }

    /// Border plus padding on one side.
    pub fn bp(&self, side: usize) -> Au {
        self.border[side] + self.padding[side]
    }

    pub fn bp_inline(&self) -> Au {
        self.bp(1) + self.bp(3)
    }

    pub fn bp_block(&self) -> Au {
        self.bp(0) + self.bp(2)
    }
}

/// A content-box size from a specified size, which `box-sizing` may say includes borders and padding.
pub fn content_size(s: &ComputedStyle, v: Au, bp: Au) -> Au {
    match s.boxm.box_sizing {
        BoxSizing::BorderBox => (v - bp).max(Au::ZERO),
        BoxSizing::ContentBox => v,
    }
}

/// Whether the box's contents are a new block formatting context, whose margins stay inside.
pub fn new_formatting_context(s: &ComputedStyle, root: bool) -> bool {
    root || s.float != Float::None
        || matches!(s.position, Position::Absolute | Position::Fixed)
        || s.place.overflow_x != Overflow::Visible
        || s.place.overflow_y != Overflow::Visible
        || !matches!(s.display, Display::Block | Display::ListItem)
}

/// A font and its metrics at its size. An inline box's content area is the ascent plus the descent as they
/// are; `line-height: normal` is the sum of the three metrics each rounded to whole px, as WebKit has it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontInfo {
    pub face: FaceId,
    pub size: f32,
    pub ascent: Au,
    pub descent: Au,
    pub x_height: Au,
    normal: Au,
}

impl FontInfo {
    pub fn normal_line_height(&self) -> Au {
        self.normal
    }
}

/// The used line height of text in `s`.
pub fn line_height(s: &ComputedStyle, f: &FontInfo) -> Au {
    match s.font.line_height {
        LineHeight::Normal => f.normal_line_height(),
        LineHeight::Number(k) => Au::from_px_trunc(f32::from_bits(k) * f.size),
        LineHeight::Px(v) => Au(v),
    }
}

// the families of a serialized family list, unquoted, the system font's aliases as `system-ui` (`ui-monospace`
// is its monospaced design), and last
// the standard font, which WebKit uses when no family of the list is installed
fn families(list: &str) -> Vec<String> {
    let mut out = Vec::new();
    for f in list.split(',') {
        let f = f.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        let f = match f.to_ascii_lowercase().as_str() {
            "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" => "system-ui".to_owned(),
            "ui-serif" | "-webkit-standard" | "-webkit-body" => "serif".to_owned(),
            _ => f.to_owned(),
        };
        if !f.is_empty() {
            out.push(f);
        }
    }
    out.push("serif".to_owned());
    out
}

// families whose ascent WebKit raises by 15% of the line, to match the metrics of their Windows counterparts
const ADJUSTED: [&str; 3] = ["Times", "Helvetica", "Courier"];

/// Font selection by computed font group; bounded by the distinct font groups of one document's styles,
/// which live as long as the layout that holds this.
#[derive(Default)]
pub struct FontTable {
    by_group: HashMap<usize, FontInfo>,
    /// By face and size: the faces of the groups and those text falls back to, at the sizes they are used
    by_face: HashMap<(FaceId, u32), FontInfo>,
}

impl FontTable {
    pub fn get(&mut self, fonts: &mut Fonts, font: &std::sync::Arc<Font>) -> FontInfo {
        let key = std::sync::Arc::as_ptr(font) as usize;
        if let Some(f) = self.by_group.get(&key) {
            return *f;
        }
        let fams = families(&font.family);
        let refs: Vec<&str> = fams.iter().map(String::as_str).collect();
        let face = fonts.select(&refs, font.weight as f32, font.style != FontStyle::Normal);
        let info = self.face(fonts, face, font.size());
        self.by_group.insert(key, info);
        info
    }

    /// The metrics of `face` at `size` px.
    pub fn face(&mut self, fonts: &Fonts, face: FaceId, size: f32) -> FontInfo {
        if let Some(f) = self.by_face.get(&(face, size.to_bits())) {
            return *f;
        }
        let m = fonts.metrics(face);
        let (mut ascent, descent, gap) = (m.ascent * size, m.descent * size, m.leading * size);
        if ADJUSTED.contains(&fonts.family_name(face)) {
            ascent += ((ascent + descent) * 0.15).round();
        }
        let info = FontInfo {
            face,
            size,
            ascent: Au::from_px(ascent),
            descent: Au::from_px(descent),
            x_height: Au::from_px(m.x_height * size),
            normal: Au::from_px(ascent.round() + descent.round() + gap.round()),
        };
        self.by_face.insert((face, size.to_bits()), info);
        info
    }
}

/// Above and below the baseline, the line height `lh` of text in `f` takes, half the leading each side.
pub fn half_leading(f: &FontInfo, lh: Au) -> (Au, Au) {
    let lead = lh - (f.ascent + f.descent);
    let top = lead.half();
    (f.ascent + top, f.descent + (lead - top))
}

// a line with text in a face other than its style's takes that face's normal line height too, as WebKit
// lays out a line whose line height is `normal`
pub fn fallback_extent(
    cx: &mut crate::cx::Cx,
    b: u32,
    shaped: Option<std::sync::Arc<madcawl_text::Shaped>>,
) -> Option<(Au, Au)> {
    let shaped = shaped?;
    let f = cx.font(b);
    if cx.style(b).font.line_height != madcawl_css::style::LineHeight::Normal {
        return None;
    }
    let mut out: Option<(Au, Au)> = None;
    let mut last = f.face;
    for g in shaped.glyphs.iter() {
        if g.face == f.face || g.face == last {
            continue;
        }
        last = g.face;
        let fi = cx.fonts.face(&cx.shaper.fonts, g.face, f.size);
        let (a, d) = half_leading(&fi, fi.normal_line_height());
        out = Some(out.map_or((a, d), |(x, y)| (x.max(a), y.max(d))));
    }
    out
}
