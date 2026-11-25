//! The initial values, and copying one property's value between
//! styles without unsharing a group that already holds it.

use std::sync::Arc;

use super::computed::*;
use super::grid::{AutoFlow, Breadth, GridLine, TrackSize};
use super::kw::*;
use super::length::Lp;
use super::props::Prop;
use super::values::{ContentVisibility, Display, Float, Position, Visibility};

const AUTO: Size = Size::Kw(SizeKw::Auto);

pub(crate) fn initial() -> ComputedStyle {
    let black = CssColor::Rgba([0, 0, 0, 255]);
    ComputedStyle {
        display: Display::Inline,
        visibility: Visibility::Visible,
        content_visibility: ContentVisibility::Visible,
        position: Position::Static,
        float: Float::None,
        opacity: 1.0f32.to_bits(),
        font: Arc::new(Font {
            family: "-webkit-standard".into(),
            size: 16.0f32.to_bits(),
            size_keyword: Some(3),
            weight: 400,
            style: FontStyle::Normal,
            line_height: LineHeight::Normal,
        }),
        text: Arc::new(Text {
            color: black,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
            text_wrap_mode: TextWrapMode::Wrap,
            text_align: TextAlign::Start,
            direction: Direction::Ltr,
            writing_mode: WritingMode::HorizontalTb,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            text_indent: Lp::ZERO,
            border_collapse: BorderCollapse::Separate,
            border_spacing: [0; 2],
            caption_side: CaptionSide::Top,
        }),
        boxm: Arc::new(BoxModel {
            margin: [Size::Lp(Lp::ZERO); 4],
            padding: [Lp::ZERO; 4],
            border_width: [0; 4],
            width: AUTO,
            height: AUTO,
            min_width: AUTO,
            min_height: AUTO,
            max_width: Size::Kw(SizeKw::None),
            max_height: Size::Kw(SizeKw::None),
            box_sizing: BoxSizing::ContentBox,
        }),
        paint: Arc::new(Paint {
            border_style: [BorderStyle::None; 4],
            border_color: std::array::from_fn(|_| CssColor::Current),
            background_color: CssColor::Rgba([0, 0, 0, 0]),
        }),
        place: Arc::new(Place {
            inset: [AUTO; 4],
            z_index: None,
            clear: Clear::None,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
        }),
        rare: Arc::new(Rare {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Nowrap,
            flex_grow: 0.0f32.to_bits(),
            flex_shrink: 1.0f32.to_bits(),
            flex_basis: AUTO,
            order: 0,
            content: super::generated::Content::Normal,
            aspect_ratio: super::generated::AspectRatio::AUTO,
            justify_content: Align::Normal,
            align_items: Align::Normal,
            align_self: Align::Auto,
            align_content: Align::Normal,
            row_gap: None,
            column_gap: None,
            vertical_align: VAlign::Kw(VerticalAlign::Baseline),
            grid_template_columns: None,
            grid_template_rows: None,
            grid_template_areas: None,
            grid_auto_columns: [TrackSize::One(Breadth::Auto)].into(),
            grid_auto_rows: [TrackSize::One(Breadth::Auto)].into(),
            grid_auto_flow: AutoFlow::NORMAL,
            grid_lines: std::array::from_fn(|_| GridLine::Auto),
            justify_items: Align::Normal,
            justify_self: Align::Auto,
            table_layout: TableLayout::Auto,
        }),
    }
}

/// Copy the fields named by each property from one style to another.
macro_rules! copy_fields {
    ($p:expr, $to:expr, $from:expr; $($prop:ident => $g:ident $(. $f:ident)* $([$i:literal])?),* $(,)?) => {
        match $p {
            // written only when it differs, so a group stays shared
            $(Prop::$prop => {
                if $to.$g$(.$f)*$([$i])? != $from.$g$(.$f)*$([$i])? {
                    Arc::make_mut(&mut $to.$g)$(.$f)*$([$i])? = $from.$g$(.$f)*$([$i])?.clone()
                }
            })*
            Prop::Display => $to.display = $from.display,
            Prop::Visibility => $to.visibility = $from.visibility,
            Prop::ContentVisibility => $to.content_visibility = $from.content_visibility,
            Prop::Opacity => $to.opacity = $from.opacity,
            Prop::Position => $to.position = $from.position,
            Prop::Float => $to.float = $from.float,
        }
    };
}

impl ComputedStyle {
    /// Take `p`'s value from `from`.
    pub(crate) fn copy_from(&mut self, from: &ComputedStyle, p: Prop) {
        copy_fields!(p, self, from;
            FontFamily => font.family,
            FontSize => font.size,
            FontWeight => font.weight,
            FontStyle => font.style,
            LineHeight => font.line_height,
            Color => text.color,
            WhiteSpaceCollapse => text.white_space_collapse,
            TextWrapMode => text.text_wrap_mode,
            TextAlign => text.text_align,
            Direction => text.direction,
            WritingMode => text.writing_mode,
            ListStyleType => text.list_style_type,
            ListStylePosition => text.list_style_position,
            TextIndent => text.text_indent,
            MarginTop => boxm.margin[0],
            MarginRight => boxm.margin[1],
            MarginBottom => boxm.margin[2],
            MarginLeft => boxm.margin[3],
            PaddingTop => boxm.padding[0],
            PaddingRight => boxm.padding[1],
            PaddingBottom => boxm.padding[2],
            PaddingLeft => boxm.padding[3],
            BorderTopWidth => boxm.border_width[0],
            BorderRightWidth => boxm.border_width[1],
            BorderBottomWidth => boxm.border_width[2],
            BorderLeftWidth => boxm.border_width[3],
            Width => boxm.width,
            Height => boxm.height,
            MinWidth => boxm.min_width,
            MinHeight => boxm.min_height,
            MaxWidth => boxm.max_width,
            MaxHeight => boxm.max_height,
            BoxSizing => boxm.box_sizing,
            BorderTopStyle => paint.border_style[0],
            BorderRightStyle => paint.border_style[1],
            BorderBottomStyle => paint.border_style[2],
            BorderLeftStyle => paint.border_style[3],
            BorderTopColor => paint.border_color[0],
            BorderRightColor => paint.border_color[1],
            BorderBottomColor => paint.border_color[2],
            BorderLeftColor => paint.border_color[3],
            BackgroundColor => paint.background_color,
            Top => place.inset[0],
            Right => place.inset[1],
            Bottom => place.inset[2],
            Left => place.inset[3],
            ZIndex => place.z_index,
            Clear => place.clear,
            OverflowX => place.overflow_x,
            OverflowY => place.overflow_y,
            FlexDirection => rare.flex_direction,
            FlexWrap => rare.flex_wrap,
            FlexGrow => rare.flex_grow,
            FlexShrink => rare.flex_shrink,
            FlexBasis => rare.flex_basis,
            Order => rare.order,
            Content => rare.content,
            AspectRatio => rare.aspect_ratio,
            JustifyContent => rare.justify_content,
            AlignItems => rare.align_items,
            AlignSelf => rare.align_self,
            AlignContent => rare.align_content,
            RowGap => rare.row_gap,
            ColumnGap => rare.column_gap,
            VerticalAlign => rare.vertical_align,
            GridTemplateColumns => rare.grid_template_columns,
            GridTemplateRows => rare.grid_template_rows,
            GridTemplateAreas => rare.grid_template_areas,
            GridAutoColumns => rare.grid_auto_columns,
            GridAutoRows => rare.grid_auto_rows,
            GridAutoFlow => rare.grid_auto_flow,
            GridRowStart => rare.grid_lines[0],
            GridColumnStart => rare.grid_lines[1],
            GridRowEnd => rare.grid_lines[2],
            GridColumnEnd => rare.grid_lines[3],
            JustifyItems => rare.justify_items,
            JustifySelf => rare.justify_self,
            TableLayout => rare.table_layout,
            BorderCollapse => text.border_collapse,
            BorderSpacing => text.border_spacing,
            CaptionSide => text.caption_side,
        );
        if p == Prop::FontSize && self.font.size_keyword != from.font.size_keyword {
            Arc::make_mut(&mut self.font).size_keyword = from.font.size_keyword;
        }
    }
}
