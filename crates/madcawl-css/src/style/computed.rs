//! One element's computed values, in groups that are shared until
//! written: an element that sets nothing in a group points at its
//! parent's (the inherited groups) or at the initial one (the others).
//! A group is cloned the first time a property in it is set.

use std::sync::{Arc, OnceLock};

use super::grid::{Areas, AutoFlow, GridLine, TrackList, TrackSize};
use super::kw::*;
use super::length::Lp;
use super::values::{ContentVisibility, Display, Float, Position, Visibility};

/// A computed color; `currentcolor` stays itself until used.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CssColor {
    Current,
    Rgba([u8; 4]),
    /// A color outside legacy sRGB, as it serializes
    Other(Arc<str>),
}

/// A size, margin or inset: a keyword or a length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Size {
    Kw(SizeKw),
    Lp(Lp),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineHeight {
    Normal,
    /// A factor of the font size, as `f32` bits
    Number(u32),
    /// 1/64 px
    Px(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VAlign {
    Kw(VerticalAlign),
    Lp(Lp),
}

/// Inherited: the font.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Font {
    /// The family list as it serializes
    pub family: Arc<str>,
    /// px, as `f32` bits
    pub(crate) size: u32,
    /// The absolute-size keyword the size comes from, if it does: a
    /// change of family to or from `monospace` changes its size
    pub size_keyword: Option<u8>,
    pub weight: u16,
    pub style: FontStyle,
    pub line_height: LineHeight,
}

impl Font {
    pub fn size(&self) -> f32 {
        f32::from_bits(self.size)
    }

    pub(crate) fn set_size(&mut self, v: f32) {
        self.size = v.max(0.0).to_bits();
    }
}

impl Text {
    /// The `white-space` keyword the two longhands make, if one.
    pub fn white_space(&self) -> Option<WhiteSpace> {
        WhiteSpace::of(self.white_space_collapse, self.text_wrap_mode)
    }
}

/// Inherited: the rest of what text takes from its ancestors.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Text {
    pub color: CssColor,
    pub white_space_collapse: WhiteSpaceCollapse,
    pub text_wrap_mode: TextWrapMode,
    pub text_align: TextAlign,
    pub direction: Direction,
    pub writing_mode: WritingMode,
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
    pub text_indent: Lp,
    pub border_collapse: BorderCollapse,
    /// Horizontal and vertical, 1/64 px
    pub border_spacing: [i32; 2],
    pub caption_side: CaptionSide,
}

/// The box model: sides in the order top, right, bottom, left.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BoxModel {
    pub margin: [Size; 4],
    pub padding: [Lp; 4],
    /// 1/64 px, after snapping, and 0 where the style is `none` or
    /// `hidden`
    pub border_width: [i32; 4],
    pub width: Size,
    pub height: Size,
    pub min_width: Size,
    pub min_height: Size,
    pub max_width: Size,
    pub max_height: Size,
    pub box_sizing: BoxSizing,
}

/// Borders and background as painted.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Paint {
    pub border_style: [BorderStyle; 4],
    pub border_color: [CssColor; 4],
    pub background_color: CssColor,
}

/// Positioning, stacking and overflow.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    pub inset: [Size; 4],
    /// `None` is `auto`
    pub z_index: Option<i32>,
    pub clear: Clear,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
}

/// What fewer elements set: flex and alignment, `vertical-align`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rare {
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    /// `f32` bits
    pub flex_grow: u32,
    pub flex_shrink: u32,
    pub flex_basis: Size,
    pub order: i32,
    pub content: super::generated::Content,
    pub aspect_ratio: super::generated::AspectRatio,
    pub justify_content: Align,
    pub align_items: Align,
    pub align_self: Align,
    pub align_content: Align,
    /// `None` is `normal`
    pub row_gap: Option<Lp>,
    pub column_gap: Option<Lp>,
    pub vertical_align: VAlign,
    /// `None` is `none`
    pub grid_template_columns: Option<TrackList<Lp>>,
    pub grid_template_rows: Option<TrackList<Lp>>,
    pub grid_template_areas: Option<Arc<Areas>>,
    pub grid_auto_columns: Arc<[TrackSize<Lp>]>,
    pub grid_auto_rows: Arc<[TrackSize<Lp>]>,
    pub grid_auto_flow: AutoFlow,
    /// Row start, column start, row end, column end, as `grid-area` orders them
    pub grid_lines: [GridLine; 4],
    pub justify_items: Align,
    pub justify_self: Align,
    pub table_layout: TableLayout,
}

/// One element's computed values. The ones that decide whether it is
/// rendered are kept in line; the rest are in shared groups.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedStyle {
    pub display: Display,
    pub visibility: Visibility,
    pub content_visibility: ContentVisibility,
    pub position: Position,
    pub float: Float,
    /// `opacity` as `f32` bits, from 0 to 1
    pub(crate) opacity: u32,
    pub font: Arc<Font>,
    pub text: Arc<Text>,
    pub boxm: Arc<BoxModel>,
    pub paint: Arc<Paint>,
    pub place: Arc<Place>,
    pub rare: Arc<Rare>,
}

impl ComputedStyle {
    /// The initial values; every call shares the same groups.
    pub fn initial() -> &'static ComputedStyle {
        static INITIAL: OnceLock<ComputedStyle> = OnceLock::new();
        INITIAL.get_or_init(super::initial::initial)
    }

    /// A child's starting point: the inherited properties from `parent`,
    /// the others initial.
    pub(crate) fn inheriting(parent: &ComputedStyle) -> ComputedStyle {
        let i = ComputedStyle::initial();
        ComputedStyle {
            visibility: parent.visibility,
            font: parent.font.clone(),
            text: parent.text.clone(),
            ..i.clone()
        }
    }

    pub fn opacity(&self) -> f32 {
        f32::from_bits(self.opacity)
    }

    pub(crate) fn set_opacity(&mut self, v: f32) {
        self.opacity = v.clamp(0.0, 1.0).to_bits();
    }

    /// Whether the groups are the very ones of `other`, not only equal.
    pub fn shares_groups(&self, other: &ComputedStyle) -> [bool; 6] {
        [
            Arc::ptr_eq(&self.font, &other.font),
            Arc::ptr_eq(&self.text, &other.text),
            Arc::ptr_eq(&self.boxm, &other.boxm),
            Arc::ptr_eq(&self.paint, &other.paint),
            Arc::ptr_eq(&self.place, &other.place),
            Arc::ptr_eq(&self.rare, &other.rare),
        ]
    }
}
