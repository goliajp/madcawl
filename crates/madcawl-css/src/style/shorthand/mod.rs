//! The shorthands of the computed longhands: which longhands each sets,
//! and their values from the shorthand's.

mod grid;
mod parts;

use super::fonts::font_shorthand;
use super::logical::{Logical, Side};
use super::longhand;
use super::props::{Prop, Specified, What};
use super::vals::Item;

use parts::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Short {
    /// `margin`, `padding`, `inset`, `border-width`, `border-style`,
    /// `border-color`: one to four values over the sides, named by the
    /// top one
    Box(Prop),
    /// `border-top` and the other three: width, style and color
    BorderSide(usize),
    Border,
    /// `margin-block` and the like: one or two values, start and end
    Pair(Prop, bool),
    /// `border-block` and `border-inline`, and their start and end
    BorderFlow(bool, Option<bool>),
    /// `border-block-width` and the like
    BorderFlowPart(Prop, bool),
    Background,
    Font,
    Flex,
    FlexFlow,
    /// `gap`: the row gap, then the column gap
    Gap,
    Overflow,
    ListStyle,
    PlaceContent,
    PlaceItems,
    PlaceSelf,
    WhiteSpace,
    /// `text-wrap`: only its mode is computed
    TextWrap,
    GridTemplate,
    Grid,
    /// `grid-row` (`true`) and `grid-column`
    GridLines(bool),
    GridArea,
}

pub(crate) fn by_name(name: &str) -> Option<Short> {
    use Prop as P;
    use Short as S;
    let flow = |s: &str| match s {
        "block" => Some(true),
        "inline" => Some(false),
        _ => None,
    };
    Some(match name {
        "margin" => S::Box(P::MarginTop),
        "padding" => S::Box(P::PaddingTop),
        "inset" => S::Box(P::Top),
        "border-width" => S::Box(P::BorderTopWidth),
        "border-style" => S::Box(P::BorderTopStyle),
        "border-color" => S::Box(P::BorderTopColor),
        "border-top" => S::BorderSide(0),
        "border-right" => S::BorderSide(1),
        "border-bottom" => S::BorderSide(2),
        "border-left" => S::BorderSide(3),
        "border" => S::Border,
        "background" => S::Background,
        "font" => S::Font,
        "flex" => S::Flex,
        "flex-flow" => S::FlexFlow,
        "gap" | "grid-gap" => S::Gap,
        "overflow" => S::Overflow,
        "list-style" => S::ListStyle,
        "place-content" => S::PlaceContent,
        "place-items" => S::PlaceItems,
        "place-self" => S::PlaceSelf,
        "white-space" => S::WhiteSpace,
        "text-wrap" => S::TextWrap,
        "grid-template" => S::GridTemplate,
        "grid" => S::Grid,
        "grid-row" => S::GridLines(true),
        "grid-column" => S::GridLines(false),
        "grid-area" => S::GridArea,
        _ => {
            let (group, rest) = name.split_once('-')?;
            let (dir, part) = rest.split_once('-').unwrap_or((rest, ""));
            let b = flow(dir)?;
            match (group, part) {
                ("margin", "") => S::Pair(P::MarginTop, b),
                ("padding", "") => S::Pair(P::PaddingTop, b),
                ("inset", "") => S::Pair(P::Top, b),
                ("border", "") => S::BorderFlow(b, None),
                ("border", "start") => S::BorderFlow(b, Some(true)),
                ("border", "end") => S::BorderFlow(b, Some(false)),
                ("border", "width") => S::BorderFlowPart(P::BorderTopWidth, b),
                ("border", "style") => S::BorderFlowPart(P::BorderTopStyle, b),
                ("border", "color") => S::BorderFlowPart(P::BorderTopColor, b),
                _ => return None,
            }
        }
    })
}

fn side(p: Prop, i: usize) -> What {
    What::Prop(Prop::ALL[p as usize + i])
}

fn flow(base: Prop, block: bool, start: bool) -> What {
    let side = match (block, start) {
        (true, true) => Side::BlockStart,
        (true, false) => Side::BlockEnd,
        (false, true) => Side::InlineStart,
        (false, false) => Side::InlineEnd,
    };
    What::Logical(Logical { base, side })
}

const WSC: [Prop; 3] = [
    Prop::BorderTopWidth,
    Prop::BorderTopStyle,
    Prop::BorderTopColor,
];

/// The longhands `sh` sets, in the order `parse` gives their values.
pub(crate) fn longhands(sh: Short) -> Vec<What> {
    use Prop as P;
    match sh {
        Short::Box(p) => (0..4).map(|i| side(p, i)).collect(),
        Short::BorderSide(i) => WSC.iter().map(|&p| side(p, i)).collect(),
        Short::Border => WSC
            .iter()
            .flat_map(|&p| (0..4).map(move |i| side(p, i)))
            .collect(),
        Short::Pair(p, b) | Short::BorderFlowPart(p, b) => {
            vec![flow(p, b, true), flow(p, b, false)]
        }
        Short::BorderFlow(b, end) => {
            let ends: &[bool] = match end {
                None => &[true, false],
                Some(true) => &[true],
                Some(false) => &[false],
            };
            WSC.iter()
                .flat_map(|&p| ends.iter().map(move |&s| flow(p, b, s)))
                .collect()
        }
        Short::Background => vec![What::Prop(P::BackgroundColor)],
        Short::Font => [
            P::FontStyle,
            P::FontWeight,
            P::FontSize,
            P::LineHeight,
            P::FontFamily,
        ]
        .map(What::Prop)
        .into(),
        Short::Flex => [P::FlexGrow, P::FlexShrink, P::FlexBasis]
            .map(What::Prop)
            .into(),
        Short::FlexFlow => [P::FlexDirection, P::FlexWrap].map(What::Prop).into(),
        Short::Gap => [P::RowGap, P::ColumnGap].map(What::Prop).into(),
        Short::Overflow => [P::OverflowX, P::OverflowY].map(What::Prop).into(),
        Short::ListStyle => [P::ListStyleType, P::ListStylePosition]
            .map(What::Prop)
            .into(),
        Short::PlaceContent => [P::AlignContent, P::JustifyContent].map(What::Prop).into(),
        Short::PlaceItems => [P::AlignItems, P::JustifyItems].map(What::Prop).into(),
        Short::PlaceSelf => [P::AlignSelf, P::JustifySelf].map(What::Prop).into(),
        Short::WhiteSpace => [P::WhiteSpaceCollapse, P::TextWrapMode]
            .map(What::Prop)
            .into(),
        Short::TextWrap => vec![What::Prop(P::TextWrapMode)],
        Short::GridTemplate => [
            P::GridTemplateRows,
            P::GridTemplateColumns,
            P::GridTemplateAreas,
        ]
        .map(What::Prop)
        .into(),
        Short::Grid => [
            P::GridTemplateRows,
            P::GridTemplateColumns,
            P::GridTemplateAreas,
            P::GridAutoRows,
            P::GridAutoColumns,
            P::GridAutoFlow,
        ]
        .map(What::Prop)
        .into(),
        Short::GridLines(row) => {
            let (s, e) = if row {
                (P::GridRowStart, P::GridRowEnd)
            } else {
                (P::GridColumnStart, P::GridColumnEnd)
            };
            [s, e].map(What::Prop).into()
        }
        Short::GridArea => [
            P::GridRowStart,
            P::GridColumnStart,
            P::GridRowEnd,
            P::GridColumnEnd,
        ]
        .map(What::Prop)
        .into(),
    }
}

/// The value of each longhand of `sh`, as `longhands` lists them.
pub(crate) fn parse(sh: Short, src: &str, xs: &[Item]) -> Option<Vec<(What, Specified)>> {
    let values = values(sh, src, xs)?;
    let names = longhands(sh);
    (values.len() == names.len()).then(|| names.into_iter().zip(values).collect())
}

pub(super) fn one(p: Prop, src: &str, x: &Item) -> Option<Specified> {
    longhand::parse(p, src, std::slice::from_ref(x))
}

// `place-items` and `place-self`: the align value is the first words that parse as one, the justify value
// the rest, or the same words when nothing is left
fn place(pa: Prop, pj: Prop, src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    (1..=xs.len()).find_map(|k| {
        let a = longhand::parse(pa, src, &xs[..k])?;
        let rest = if k == xs.len() { &xs[..k] } else { &xs[k..] };
        Some(vec![a, longhand::parse(pj, src, rest)?])
    })
}

fn values(sh: Short, src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    use Prop as P;
    Some(match sh {
        Short::Box(p) => {
            let v: Vec<Specified> = xs.iter().map(|x| one(p, src, x)).collect::<Option<_>>()?;
            let [t, r, b, l] = match &v[..] {
                [a] => [a, a, a, a],
                [a, b] => [a, b, a, b],
                [a, b, c] => [a, b, c, b],
                [a, b, c, d] => [a, b, c, d],
                _ => return None,
            };
            vec![t.clone(), r.clone(), b.clone(), l.clone()]
        }
        Short::Pair(p, _) | Short::BorderFlowPart(p, _) => {
            let v: Vec<Specified> = xs.iter().map(|x| one(p, src, x)).collect::<Option<_>>()?;
            match &v[..] {
                [a] => vec![a.clone(), a.clone()],
                [a, b] => vec![a.clone(), b.clone()],
                _ => return None,
            }
        }
        Short::BorderSide(_) => wsc(src, xs)?.into(),
        Short::Border => wsc(src, xs)?
            .into_iter()
            .flat_map(|v| std::iter::repeat_n(v, 4))
            .collect(),
        Short::BorderFlow(_, end) => {
            let n = if end.is_none() { 2 } else { 1 };
            wsc(src, xs)?
                .into_iter()
                .flat_map(|v| std::iter::repeat_n(v, n))
                .collect()
        }
        Short::Background => vec![background(src, xs)?],
        Short::Font => font_shorthand(src, xs)?,
        Short::Flex => flex(src, xs)?,
        Short::FlexFlow => flex_flow(src, xs)?,
        Short::Gap => {
            let v: Vec<Specified> = xs
                .iter()
                .map(|x| one(Prop::RowGap, src, x))
                .collect::<Option<_>>()?;
            match &v[..] {
                [a] => vec![a.clone(), a.clone()],
                [a, b] => vec![a.clone(), b.clone()],
                _ => return None,
            }
        }
        Short::Overflow => match xs {
            [a] => {
                let v = one(P::OverflowX, src, a)?;
                vec![v.clone(), v]
            }
            [a, b] => vec![one(P::OverflowX, src, a)?, one(P::OverflowY, src, b)?],
            _ => return None,
        },
        Short::ListStyle => list_style(src, xs)?,
        Short::PlaceContent => match xs {
            [a] => {
                let v = one(P::AlignContent, src, a)?;
                let j = one(P::JustifyContent, src, a)
                    .unwrap_or(Specified::Align(super::kw::Align::Start));
                vec![v, j]
            }
            [a, b] => vec![
                one(P::AlignContent, src, a)?,
                one(P::JustifyContent, src, b)?,
            ],
            _ => return None,
        },
        Short::PlaceItems => place(P::AlignItems, P::JustifyItems, src, xs)?,
        Short::WhiteSpace => white_space(src, xs)?,
        Short::TextWrap => text_wrap(src, xs)?,
        Short::PlaceSelf => place(P::AlignSelf, P::JustifySelf, src, xs)?,
        Short::GridTemplate => grid::grid_template(src, xs)?,
        Short::Grid => grid::grid(src, xs)?,
        Short::GridLines(_) => grid::line_pair(src, xs)?,
        Short::GridArea => grid::area(src, xs)?,
    })
}
