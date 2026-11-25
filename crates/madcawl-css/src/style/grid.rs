//! The values of the grid properties, declared (lengths of type `SpecLen`) and computed (`Lp`): track
//! lists with their line names and `repeat()`, the template areas, and grid lines.

use std::fmt::Write;
use std::sync::Arc;

/// The most tracks a `repeat()` with a count, a template, or an area string may give; a value past it does
/// not parse. Layout caps the implicit grid at the same count.
pub const MAX_TRACKS: usize = 1000;

/// A track's minimum or maximum.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Breadth<L> {
    Len(L),
    /// A flexible length, as `f32` bits
    Fr(u32),
    Auto,
    MinContent,
    MaxContent,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TrackSize<L> {
    One(Breadth<L>),
    MinMax(Breadth<L>, Breadth<L>),
    FitContent(L),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Repeat {
    Count(u16),
    AutoFill,
    AutoFit,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TrackEntry<L> {
    /// The names of the line between the tracks around it
    Names(Arc<[Arc<str>]>),
    Size(TrackSize<L>),
    Repeat(Repeat, Arc<[TrackEntry<L>]>),
}

/// An explicit track list: line names, track sizes and repeats in the order written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackList<L>(pub Arc<[TrackEntry<L>]>);

/// `grid-template-areas`: the strings, and each named area's lines from 0, row start, row end, column
/// start, column end.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Areas {
    pub rows: u16,
    pub cols: u16,
    pub strings: Arc<[Arc<str>]>,
    pub named: Arc<[(Arc<str>, [u16; 4])]>,
}

/// A `grid-row-start` and the like.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GridLine {
    Auto,
    /// A named line or area
    Ident(Arc<str>),
    /// The nth line, or the nth one of the name, counted from the end when negative
    Line(i32, Option<Arc<str>>),
    Span(u32, Option<Arc<str>>),
}

/// `grid-auto-flow`; `normal` lays out as `row`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AutoFlow {
    pub normal: bool,
    pub column: bool,
    pub dense: bool,
}

impl AutoFlow {
    pub const NORMAL: AutoFlow = AutoFlow {
        normal: true,
        column: false,
        dense: false,
    };
}

impl<L> Breadth<L> {
    fn map<M>(&self, f: &mut impl FnMut(&L) -> Option<M>) -> Option<Breadth<M>> {
        Some(match self {
            Breadth::Len(l) => Breadth::Len(f(l)?),
            Breadth::Fr(v) => Breadth::Fr(*v),
            Breadth::Auto => Breadth::Auto,
            Breadth::MinContent => Breadth::MinContent,
            Breadth::MaxContent => Breadth::MaxContent,
        })
    }

    pub fn fr(&self) -> Option<f32> {
        match self {
            Breadth::Fr(v) => Some(f32::from_bits(*v)),
            _ => None,
        }
    }
}

impl<L> TrackSize<L> {
    pub fn map<M>(&self, f: &mut impl FnMut(&L) -> Option<M>) -> Option<TrackSize<M>> {
        Some(match self {
            TrackSize::One(b) => TrackSize::One(b.map(f)?),
            TrackSize::MinMax(a, b) => TrackSize::MinMax(a.map(f)?, b.map(f)?),
            TrackSize::FitContent(l) => TrackSize::FitContent(f(l)?),
        })
    }
}

fn map_entries<L, M>(
    es: &[TrackEntry<L>],
    f: &mut impl FnMut(&L) -> Option<M>,
) -> Option<Arc<[TrackEntry<M>]>> {
    es.iter()
        .map(|e| {
            Some(match e {
                TrackEntry::Names(n) => TrackEntry::Names(n.clone()),
                TrackEntry::Size(s) => TrackEntry::Size(s.map(f)?),
                TrackEntry::Repeat(r, inner) => TrackEntry::Repeat(*r, map_entries(inner, f)?),
            })
        })
        .collect()
}

impl<L> TrackList<L> {
    pub fn map<M>(&self, f: &mut impl FnMut(&L) -> Option<M>) -> Option<TrackList<M>> {
        Some(TrackList(map_entries(&self.0, f)?))
    }
}

// written as `getComputedStyle` writes them where it does not give the used track sizes
pub(crate) fn write_size<L>(s: &TrackSize<L>, out: &mut String, len: &impl Fn(&L, &mut String)) {
    match s {
        TrackSize::One(b) => write_breadth(b, out, len),
        TrackSize::MinMax(a, b) => {
            out.push_str("minmax(");
            write_breadth(a, out, len);
            out.push_str(", ");
            write_breadth(b, out, len);
            out.push(')');
        }
        TrackSize::FitContent(l) => {
            out.push_str("fit-content(");
            len(l, out);
            out.push(')');
        }
    }
}

fn write_breadth<L>(b: &Breadth<L>, out: &mut String, len: &impl Fn(&L, &mut String)) {
    match b {
        Breadth::Len(v) => len(v, out),
        Breadth::Fr(v) => {
            super::cssom::number(f64::from(f32::from_bits(*v)), out);
            out.push_str("fr");
        }
        Breadth::Auto => out.push_str("auto"),
        Breadth::MinContent => out.push_str("min-content"),
        Breadth::MaxContent => out.push_str("max-content"),
    }
}

pub(crate) fn write_entries<L>(
    es: &[TrackEntry<L>],
    out: &mut String,
    len: &impl Fn(&L, &mut String),
) {
    for (k, e) in es.iter().enumerate() {
        if k > 0 {
            out.push(' ');
        }
        match e {
            TrackEntry::Names(n) => {
                out.push('[');
                for (j, x) in n.iter().enumerate() {
                    if j > 0 {
                        out.push(' ');
                    }
                    out.push_str(x);
                }
                out.push(']');
            }
            TrackEntry::Size(s) => write_size(s, out, len),
            TrackEntry::Repeat(r, inner) => {
                out.push_str("repeat(");
                match r {
                    Repeat::Count(n) => {
                        let _ = write!(out, "{n}");
                    }
                    Repeat::AutoFill => out.push_str("auto-fill"),
                    Repeat::AutoFit => out.push_str("auto-fit"),
                }
                out.push_str(", ");
                write_entries(inner, out, len);
                out.push(')');
            }
        }
    }
}

pub(crate) fn write_line(g: &GridLine, out: &mut String) {
    match g {
        GridLine::Auto => out.push_str("auto"),
        GridLine::Ident(n) => out.push_str(n),
        GridLine::Line(n, name) => {
            let _ = write!(out, "{n}");
            if let Some(name) = name {
                out.push(' ');
                out.push_str(name);
            }
        }
        GridLine::Span(n, name) => {
            out.push_str("span");
            if *n != 1 || name.is_none() {
                let _ = write!(out, " {n}");
            }
            if let Some(name) = name {
                out.push(' ');
                out.push_str(name);
            }
        }
    }
}

pub(crate) fn write_areas(a: &Option<Arc<Areas>>, out: &mut String) {
    let Some(a) = a else {
        return out.push_str("none");
    };
    for (k, s) in a.strings.iter().enumerate() {
        if k > 0 {
            out.push(' ');
        }
        out.push('"');
        out.push_str(s);
        out.push('"');
    }
}

pub(crate) fn write_flow(f: AutoFlow, grid: bool, out: &mut String) {
    out.push_str(match (f.normal && !grid, f.column, f.dense) {
        (true, _, _) => "normal",
        (_, false, false) => "row",
        (_, false, true) => "dense",
        (_, true, false) => "column",
        (_, true, true) => "column dense",
    });
}
