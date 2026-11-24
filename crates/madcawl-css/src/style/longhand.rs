//! The declared values of the longhands, from a value's items.

use crate::tokenizer::Kind;

use super::fonts::{family, font_size, line_height, weight};
use super::kw::*;
use super::length::{Accept, parse_len, px};
use super::props::{Prop, Specified};

pub(crate) use super::colors::color;
use super::vals::Item;
use super::values::{ContentVisibility, Display, Float, Position, Visibility};

const ANY: Accept = Accept {
    pct: true,
    neg: true,
};
const POSITIVE: Accept = Accept {
    pct: true,
    neg: false,
};
const LENGTH_ONLY: Accept = Accept {
    pct: false,
    neg: false,
};

/// The declared value of `p` in `xs`, CSS-wide keywords and `var()`
/// already handled.
pub(crate) fn parse(p: Prop, src: &str, xs: &[Item]) -> Option<Specified> {
    use Prop as P;
    if let Some(v) = multi(p, src, xs) {
        return Some(v);
    }
    let [x] = xs else { return None };
    let id = x.ident(src);
    let kw = id.as_deref();
    Some(match p {
        P::Visibility => Specified::Visibility(Visibility::from_keyword(kw?)?),
        P::ContentVisibility => Specified::ContentVisibility(ContentVisibility::from_keyword(kw?)?),
        P::Position => {
            let w = kw?;
            Specified::Position(Position::from_keyword(if w == "-webkit-sticky" {
                "sticky"
            } else {
                w
            })?)
        }
        P::Float => Specified::Float(Float::from_keyword(kw?)?),
        P::Opacity => Specified::Number(number_or_pct(src, x)?),
        P::MarginTop | P::MarginRight | P::MarginBottom | P::MarginLeft => {
            auto_or(src, x, kw, ANY)?
        }
        P::Top | P::Right | P::Bottom | P::Left => auto_or(src, x, kw, ANY)?,
        P::PaddingTop | P::PaddingRight | P::PaddingBottom | P::PaddingLeft => {
            Specified::Len(parse_len(src, x, POSITIVE)?)
        }
        P::BorderTopWidth | P::BorderRightWidth | P::BorderBottomWidth | P::BorderLeftWidth => {
            border_width(src, x, kw)?
        }
        P::Width | P::Height | P::MinWidth | P::MinHeight | P::MaxWidth | P::MaxHeight => {
            size(p, src, x, kw)?
        }
        P::FlexBasis => match kw {
            Some("auto") => Specified::Size(SizeKw::Auto),
            Some("content") => Specified::Size(SizeKw::Content),
            _ => size(P::Width, src, x, kw)?,
        },
        P::BoxSizing => Specified::BoxSizing(BoxSizing::from_keyword(kw?)?),
        P::BorderTopStyle | P::BorderRightStyle | P::BorderBottomStyle | P::BorderLeftStyle => {
            Specified::BorderStyle(BorderStyle::from_keyword(kw?)?)
        }
        P::BorderTopColor
        | P::BorderRightColor
        | P::BorderBottomColor
        | P::BorderLeftColor
        | P::BackgroundColor
        | P::Color => Specified::Color(color(src, x)?),
        P::ZIndex => match kw {
            Some("auto") => Specified::Auto,
            _ => Specified::Integer(integer(src, x)?),
        },
        P::Order => Specified::Integer(integer(src, x)?),
        P::Clear => Specified::Clear(Clear::from_keyword(kw?)?),
        P::OverflowX | P::OverflowY => Specified::Overflow(overflow(kw?)?),
        P::FontFamily => family(src, xs)?,
        P::FontSize => font_size(src, x, kw)?,
        P::FontWeight => Specified::Weight(weight(src, x, kw)?),
        P::FontStyle => Specified::FontStyle(FontStyle::from_keyword(kw?)?),
        P::LineHeight => line_height(src, x, kw)?,
        P::WhiteSpaceCollapse => {
            Specified::WhiteSpaceCollapse(WhiteSpaceCollapse::from_keyword(kw?)?)
        }
        P::TextWrapMode => Specified::TextWrapMode(TextWrapMode::from_keyword(kw?)?),
        P::TextAlign => Specified::TextAlign(text_align(kw?)?),
        P::Direction => Specified::Direction(Direction::from_keyword(kw?)?),
        P::WritingMode => Specified::WritingMode(WritingMode::parse(kw?)?),
        P::ListStyleType => Specified::ListStyleType(ListStyleType::from_keyword(kw?)?),
        P::ListStylePosition => Specified::ListStylePosition(ListStylePosition::from_keyword(kw?)?),
        P::TextIndent => Specified::Len(parse_len(src, x, ANY)?),
        P::FlexDirection => Specified::FlexDirection(FlexDirection::from_keyword(kw?)?),
        P::FlexWrap => Specified::FlexWrap(FlexWrap::from_keyword(kw?)?),
        P::FlexGrow | P::FlexShrink => Specified::Number(non_negative(src, x)?),
        P::JustifyContent | P::AlignItems | P::AlignSelf | P::AlignContent => align(p, kw?)?,
        P::VerticalAlign => match kw {
            Some(w) => Specified::VAlign(VerticalAlign::from_keyword(w)?),
            None => Specified::Len(parse_len(src, x, ANY)?),
        },
        P::Display => return None,
    })
}

/// The values of more than one item.
fn multi(p: Prop, src: &str, xs: &[Item]) -> Option<Specified> {
    match p {
        Prop::Display => {
            let ks: Option<Vec<String>> = xs.iter().map(|x| x.ident(src)).collect();
            let ks = ks?;
            let ks: Vec<&str> = ks.iter().map(String::as_str).collect();
            Some(Specified::Display(Display::from_keywords(&ks)?))
        }
        Prop::FontFamily => family(src, xs),
        Prop::JustifyContent | Prop::AlignItems | Prop::AlignSelf | Prop::AlignContent => {
            let [a, b] = xs else { return None };
            let (a, b) = (a.ident(src)?, b.ident(src)?);
            match (a.as_str(), b.as_str()) {
                ("first", "baseline") if p != Prop::JustifyContent => {
                    Some(Specified::Align(Align::Baseline))
                }
                ("last", "baseline") if p != Prop::JustifyContent => {
                    Some(Specified::Align(Align::LastBaseline))
                }
                // the overflow position is not kept
                ("safe" | "unsafe", w) => match align(p, w)? {
                    Specified::Align(Align::Normal | Align::Stretch | Align::Auto) => None,
                    Specified::Align(
                        Align::Baseline
                        | Align::LastBaseline
                        | Align::SpaceAround
                        | Align::SpaceBetween
                        | Align::SpaceEvenly,
                    ) => None,
                    v => Some(v),
                },
                _ => None,
            }
        }
        _ => None,
    }
}

fn number_or_pct(src: &str, x: &Item) -> Option<f32> {
    Some(match x.kind() {
        Kind::Number => x.tok.number(src) as f32,
        Kind::Percentage => x.tok.number(src) as f32 / 100.0,
        _ => return None,
    })
}

pub(crate) fn non_negative(src: &str, x: &Item) -> Option<f32> {
    let v = (x.kind() == Kind::Number).then(|| x.tok.number(src) as f32)?;
    (v >= 0.0).then_some(v)
}

fn integer(src: &str, x: &Item) -> Option<i32> {
    (x.kind() == Kind::Number && x.tok.is(crate::INTEGER))
        .then(|| x.tok.number(src).clamp(i32::MIN as f64, i32::MAX as f64) as i32)
}

fn auto_or(src: &str, x: &Item, kw: Option<&str>, a: Accept) -> Option<Specified> {
    match kw {
        Some("auto") => Some(Specified::Size(SizeKw::Auto)),
        _ => Some(Specified::Len(parse_len(src, x, a)?)),
    }
}

pub(crate) fn border_width(src: &str, x: &Item, kw: Option<&str>) -> Option<Specified> {
    Some(Specified::Len(match kw {
        Some("thin") => px(1.0),
        Some("medium") => px(3.0),
        Some("thick") => px(5.0),
        _ => parse_len(src, x, LENGTH_ONLY)?,
    }))
}

fn size(p: Prop, src: &str, x: &Item, kw: Option<&str>) -> Option<Specified> {
    let max = matches!(p, Prop::MaxWidth | Prop::MaxHeight);
    if let Some(w) = kw {
        let k = match w {
            "-webkit-min-content" => SizeKw::MinContent,
            "-webkit-max-content" => SizeKw::MaxContent,
            "-webkit-fit-content" => SizeKw::FitContent,
            "stretch" => SizeKw::Stretch,
            _ => SizeKw::from_keyword(w)?,
        };
        let ok = match k {
            SizeKw::Auto => !max,
            SizeKw::None => max,
            SizeKw::Content => false,
            _ => true,
        };
        return ok.then_some(Specified::Size(k));
    }
    Some(Specified::Len(parse_len(src, x, POSITIVE)?))
}

fn overflow(w: &str) -> Option<Overflow> {
    match w {
        "overlay" => Some(Overflow::Auto),
        _ => Overflow::from_keyword(w),
    }
}

fn text_align(w: &str) -> Option<TextAlign> {
    match w {
        "-webkit-match-parent" => Some(TextAlign::MatchParent),
        "-webkit-auto" => Some(TextAlign::Start),
        _ => TextAlign::from_keyword(w),
    }
}

fn align(p: Prop, w: &str) -> Option<Specified> {
    let a = Align::from_keyword(w)?;
    use Align as A;
    let ok = match p {
        Prop::JustifyContent => matches!(
            a,
            A::Normal
                | A::Stretch
                | A::Center
                | A::Start
                | A::End
                | A::FlexStart
                | A::FlexEnd
                | A::Left
                | A::Right
                | A::SpaceBetween
                | A::SpaceAround
                | A::SpaceEvenly
        ),
        Prop::AlignContent => {
            !matches!(a, A::Auto | A::SelfStart | A::SelfEnd | A::Left | A::Right)
        }
        Prop::AlignItems => !matches!(
            a,
            A::Auto | A::Left | A::Right | A::SpaceBetween | A::SpaceAround | A::SpaceEvenly
        ),
        _ => !matches!(
            a,
            A::Left | A::Right | A::SpaceBetween | A::SpaceAround | A::SpaceEvenly
        ),
    };
    ok.then_some(Specified::Align(a))
}
