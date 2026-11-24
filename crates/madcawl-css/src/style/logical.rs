//! The flow-relative properties: which physical property each one sets
//! on an element, by its writing mode and direction. A flow-relative
//! declaration and a physical one for the same side take part in one
//! cascade, the later winning as usual.

use super::kw::{Direction, WritingMode};
use super::props::Prop;

/// A flow-relative side, or one of the two flow-relative sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    BlockStart,
    BlockEnd,
    InlineStart,
    InlineEnd,
    /// `inline-size`
    Inline,
    /// `block-size`
    Block,
}

/// A flow-relative longhand: the physical group it belongs to, named by
/// its first member (`margin-top`, `width`), and the side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Logical {
    pub base: Prop,
    pub side: Side,
}

/// The flow-relative longhand an ASCII-lowercase name is.
pub(crate) fn logical_name(name: &str) -> Option<Logical> {
    let sizes = [
        ("inline-size", Prop::Width, Side::Inline),
        ("block-size", Prop::Width, Side::Block),
        ("min-inline-size", Prop::MinWidth, Side::Inline),
        ("min-block-size", Prop::MinWidth, Side::Block),
        ("max-inline-size", Prop::MaxWidth, Side::Inline),
        ("max-block-size", Prop::MaxWidth, Side::Block),
    ];
    if let Some(&(_, base, side)) = sizes.iter().find(|s| s.0 == name) {
        return Some(Logical { base, side });
    }
    let (group, rest) = name.split_once('-')?;
    let (rest, suffix) = match group {
        "border" => rest.rsplit_once('-')?,
        _ => (rest, ""),
    };
    let base = match (group, suffix) {
        ("margin", "") => Prop::MarginTop,
        ("padding", "") => Prop::PaddingTop,
        ("inset", "") => Prop::Top,
        ("border", "width") => Prop::BorderTopWidth,
        ("border", "style") => Prop::BorderTopStyle,
        ("border", "color") => Prop::BorderTopColor,
        _ => return None,
    };
    let side = match rest {
        "block-start" => Side::BlockStart,
        "block-end" => Side::BlockEnd,
        "inline-start" => Side::InlineStart,
        "inline-end" => Side::InlineEnd,
        _ => return None,
    };
    Some(Logical { base, side })
}

/// The physical property `l` sets on an element with writing mode `wm`
/// and direction `dir`.
pub(crate) fn physical(l: Logical, wm: WritingMode, dir: Direction) -> Prop {
    const TOP: usize = 0;
    const RIGHT: usize = 1;
    const BOTTOM: usize = 2;
    const LEFT: usize = 3;
    let ltr = dir == Direction::Ltr;
    let (bs, be, is) = match wm {
        WritingMode::HorizontalTb => (TOP, BOTTOM, if ltr { LEFT } else { RIGHT }),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            (RIGHT, LEFT, if ltr { TOP } else { BOTTOM })
        }
        WritingMode::VerticalLr => (LEFT, RIGHT, if ltr { TOP } else { BOTTOM }),
        WritingMode::SidewaysLr => (LEFT, RIGHT, if ltr { BOTTOM } else { TOP }),
    };
    let horizontal = !wm.vertical();
    let offset = match l.side {
        Side::BlockStart => bs,
        Side::BlockEnd => be,
        Side::InlineStart => is,
        Side::InlineEnd => (is + 2) % 4,
        Side::Inline => usize::from(!horizontal),
        Side::Block => usize::from(horizontal),
    };
    Prop::ALL[l.base as usize + offset]
}
