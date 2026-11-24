//! The properties the cascade knows, and their declared values parsed
//! from a declaration's value tokens.

use std::sync::Arc;

use crate::Span;
use crate::tokenizer::Kind;

use super::kw::*;
use super::length::SpecLen;
use super::vals::items;
use super::values::{ContentVisibility, Display, Float, Position, Visibility};
use super::{longhand, tokens, var};

pub(crate) use super::decls::declaration_parses;
pub use super::decls::{Decl, What, parse_block};

macro_rules! props {
    ($($v:ident = $s:literal, $inherited:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Prop {
            $($v),*
        }

        impl Prop {
            pub const ALL: &[Prop] = &[$(Prop::$v),*];

            pub fn name(self) -> &'static str {
                match self {
                    $(Prop::$v => $s),*
                }
            }

            /// The one named by an ASCII-lowercase name.
            pub fn from_name(name: &str) -> Option<Prop> {
                match name {
                    $($s => Some(Prop::$v),)*
                    _ => None,
                }
            }

            pub fn inherited(self) -> bool {
                match self {
                    $(Prop::$v => $inherited),*
                }
            }
        }
    };
}

// the four sides of a box property are in the order top, right, bottom,
// left, and each width comes before its height, which the flow-relative
// mapping counts on; the font comes first, since lengths in em resolve
// against its size
props! {
    FontFamily = "font-family", true;
    FontSize = "font-size", true;
    FontWeight = "font-weight", true;
    FontStyle = "font-style", true;
    LineHeight = "line-height", true;
    Display = "display", false;
    Visibility = "visibility", true;
    ContentVisibility = "content-visibility", false;
    Opacity = "opacity", false;
    Position = "position", false;
    Float = "float", false;
    Color = "color", true;
    WhiteSpaceCollapse = "white-space-collapse", true;
    TextWrapMode = "text-wrap-mode", true;
    TextAlign = "text-align", true;
    Direction = "direction", true;
    WritingMode = "writing-mode", true;
    ListStyleType = "list-style-type", true;
    ListStylePosition = "list-style-position", true;
    TextIndent = "text-indent", true;
    MarginTop = "margin-top", false;
    MarginRight = "margin-right", false;
    MarginBottom = "margin-bottom", false;
    MarginLeft = "margin-left", false;
    PaddingTop = "padding-top", false;
    PaddingRight = "padding-right", false;
    PaddingBottom = "padding-bottom", false;
    PaddingLeft = "padding-left", false;
    BorderTopWidth = "border-top-width", false;
    BorderRightWidth = "border-right-width", false;
    BorderBottomWidth = "border-bottom-width", false;
    BorderLeftWidth = "border-left-width", false;
    Width = "width", false;
    Height = "height", false;
    MinWidth = "min-width", false;
    MinHeight = "min-height", false;
    MaxWidth = "max-width", false;
    MaxHeight = "max-height", false;
    BoxSizing = "box-sizing", false;
    BorderTopStyle = "border-top-style", false;
    BorderRightStyle = "border-right-style", false;
    BorderBottomStyle = "border-bottom-style", false;
    BorderLeftStyle = "border-left-style", false;
    BorderTopColor = "border-top-color", false;
    BorderRightColor = "border-right-color", false;
    BorderBottomColor = "border-bottom-color", false;
    BorderLeftColor = "border-left-color", false;
    BackgroundColor = "background-color", false;
    Top = "top", false;
    Right = "right", false;
    Bottom = "bottom", false;
    Left = "left", false;
    ZIndex = "z-index", false;
    Clear = "clear", false;
    OverflowX = "overflow-x", false;
    OverflowY = "overflow-y", false;
    FlexDirection = "flex-direction", false;
    FlexWrap = "flex-wrap", false;
    FlexGrow = "flex-grow", false;
    FlexShrink = "flex-shrink", false;
    FlexBasis = "flex-basis", false;
    Order = "order", false;
    JustifyContent = "justify-content", false;
    AlignItems = "align-items", false;
    AlignSelf = "align-self", false;
    AlignContent = "align-content", false;
    VerticalAlign = "vertical-align", false;
}

pub const PROPS: usize = Prop::ALL.len();

/// The CSS-wide keywords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wide {
    Initial,
    Inherit,
    Unset,
    Revert,
    RevertLayer,
}

/// A declared color.
#[derive(Clone, Debug, PartialEq)]
pub enum SpecColor {
    Current,
    Rgba([u8; 4]),
    /// `light-dark()`, the light color first
    LightDark([u8; 4], [u8; 4]),
    /// A color outside legacy sRGB, as it serializes
    Other(Arc<str>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum FontSize {
    /// `xx-small` to `xxx-large`
    Keyword(u8),
    Smaller,
    Larger,
    Len(SpecLen),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Weight {
    Abs(u16),
    Bolder,
    Lighter,
}

/// A declared value.
#[derive(Clone, Debug, PartialEq)]
pub enum Specified {
    Wide(Wide),
    Display(Display),
    Visibility(Visibility),
    ContentVisibility(ContentVisibility),
    Position(Position),
    Float(Float),
    BoxSizing(BoxSizing),
    BorderStyle(BorderStyle),
    Overflow(Overflow),
    Clear(Clear),
    WhiteSpaceCollapse(WhiteSpaceCollapse),
    TextWrapMode(TextWrapMode),
    TextAlign(TextAlign),
    Direction(Direction),
    WritingMode(WritingMode),
    ListStyleType(ListStyleType),
    ListStylePosition(ListStylePosition),
    FontStyle(FontStyle),
    FlexDirection(FlexDirection),
    FlexWrap(FlexWrap),
    Align(Align),
    VAlign(VerticalAlign),
    Len(SpecLen),
    Size(SizeKw),
    Number(f32),
    Integer(i32),
    /// `auto` of `z-index`
    Auto,
    /// `normal` of `line-height`
    Normal,
    Color(SpecColor),
    Family(Arc<str>),
    FontSize(FontSize),
    Weight(Weight),
    /// Holds `var()`, substituted at computed-value time; the text is
    /// the declaration's `raw`
    Unresolved,
}

/// The CSS-wide keyword `src[value]` is, if it is one.
pub(crate) fn wide(src: &str, value: Span) -> Option<Wide> {
    let t = tokens(src, value);
    let [t] = &t[..] else { return None };
    if t.kind != Kind::Ident {
        return None;
    }
    Some(match () {
        _ if t.value_is(src, "initial") => Wide::Initial,
        _ if t.value_is(src, "inherit") => Wide::Inherit,
        _ if t.value_is(src, "unset") => Wide::Unset,
        _ if t.value_is(src, "revert") => Wide::Revert,
        _ if t.value_is(src, "revert-layer") => Wide::RevertLayer,
        _ => return None,
    })
}

pub(crate) fn has_var(src: &str, value: Span) -> bool {
    tokens(src, value)
        .iter()
        .any(|t| t.kind == Kind::Function && (t.value_is(src, "var") || t.value_is(src, "env")))
}

/// The declared value of `prop` in `src[value]`; `None` when it is not
/// one.
pub fn parse(prop: Prop, src: &str, value: Span) -> Option<Specified> {
    if has_var(src, value) {
        return var::vars_valid(src, value).then_some(Specified::Unresolved);
    }
    if let Some(w) = wide(src, value) {
        return Some(Specified::Wide(w));
    }
    longhand::parse(prop, src, &items(src, value, 64)?)
}
