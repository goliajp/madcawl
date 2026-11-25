//! Parsing the grid properties and their shorthands from a value's items.

use std::sync::Arc;

use crate::tokenizer::Kind;

use super::grid::{
    Areas, AutoFlow, Breadth, GridLine, MAX_TRACKS, Repeat, TrackEntry, TrackList, TrackSize,
};
use super::length::{Accept, SpecLen, parse_len};
use super::props::Specified;
use super::vals::{Item, items};

const POSITIVE: Accept = Accept {
    pct: true,
    neg: false,
};

type Spec = TrackSize<SpecLen>;

fn raw(src: &str, x: &Item) -> String {
    let mut s = String::new();
    x.tok.write_value(src, &mut s);
    s
}

/// A `<custom-ident>` as written, which grid names keep the case of.
fn custom_ident(src: &str, x: &Item) -> Option<Arc<str>> {
    let lower = x.ident(src)?;
    let reserved = [
        "span", "auto", "initial", "inherit", "unset", "default", "revert",
    ];
    (!reserved.contains(&lower.as_str())).then(|| raw(src, x).into())
}

fn inner(src: &str, x: &Item) -> Option<Vec<Item>> {
    items(src, x.inner, 64)
}

fn breadth(src: &str, x: &Item, fr: bool) -> Option<Breadth<SpecLen>> {
    if let Some(w) = x.ident(src) {
        return Some(match w.as_str() {
            "auto" => Breadth::Auto,
            "min-content" | "-webkit-min-content" => Breadth::MinContent,
            "max-content" | "-webkit-max-content" => Breadth::MaxContent,
            _ => return None,
        });
    }
    if x.kind() == Kind::Dimension && x.tok.unit_is(src, "fr") {
        let v = x.tok.number(src) as f32;
        return (fr && v >= 0.0 && v.is_finite()).then(|| Breadth::Fr(v.to_bits()));
    }
    parse_len(src, x, POSITIVE).map(Breadth::Len)
}

/// One `<track-size>`.
pub(crate) fn track_size(src: &str, x: &Item) -> Option<Spec> {
    if x.is_function(src, "minmax") {
        let xs = inner(src, x)?;
        let [a, c, b] = &xs[..] else { return None };
        if !c.is_comma() {
            return None;
        }
        let min = breadth(src, a, false)?;
        return Some(TrackSize::MinMax(min, breadth(src, b, true)?));
    }
    if x.is_function(src, "fit-content") {
        let xs = inner(src, x)?;
        let [a] = &xs[..] else { return None };
        return Some(TrackSize::FitContent(parse_len(src, a, POSITIVE)?));
    }
    Some(TrackSize::One(breadth(src, x, true)?))
}

fn fixed(s: &Spec) -> bool {
    let f = |b: &Breadth<SpecLen>| matches!(b, Breadth::Len(_));
    match s {
        TrackSize::One(b) => f(b),
        TrackSize::MinMax(a, b) => f(a) || f(b) && !matches!(a, Breadth::Fr(_)),
        TrackSize::FitContent(_) => false,
    }
}

/// `[name ...]`.
pub(crate) fn names(src: &str, x: &Item) -> Option<Arc<[Arc<str>]>> {
    if x.kind() != Kind::OpenSquare {
        return None;
    }
    let xs = inner(src, x)?;
    xs.iter().map(|n| custom_ident(src, n)).collect()
}

/// Entries of a track list and how many tracks they give.
type Entries = (Vec<TrackEntry<SpecLen>>, usize);

// the entries of a track list or of a repeat's tracks
fn entries(src: &str, xs: &[Item], in_repeat: bool) -> Option<Entries> {
    let (mut out, mut n, mut auto_repeats) = (Vec::new(), 0usize, 0);
    for x in xs {
        if let Some(ns) = names(src, x) {
            if matches!(out.last(), Some(TrackEntry::Names(_))) {
                return None;
            }
            out.push(TrackEntry::Names(ns));
        } else if x.is_function(src, "repeat") && !in_repeat {
            let (r, inner) = repeat(src, x)?;
            let count = match r {
                Repeat::Count(c) => usize::from(c),
                _ => {
                    auto_repeats += 1;
                    1
                }
            };
            n = n.saturating_add(count.saturating_mul(inner.1));
            out.push(TrackEntry::Repeat(r, inner.0.into()));
        } else {
            out.push(TrackEntry::Size(track_size(src, x)?));
            n += 1;
        }
    }
    let ok = auto_repeats <= 1 && (n > 0 || in_repeat);
    ok.then_some((out, n))
}

fn repeat(src: &str, x: &Item) -> Option<(Repeat, Entries)> {
    let xs = inner(src, x)?;
    let comma = xs.iter().position(Item::is_comma)?;
    let [c] = &xs[..comma] else { return None };
    let r = match c.ident(src).as_deref() {
        Some("auto-fill") => Repeat::AutoFill,
        Some("auto-fit") => Repeat::AutoFit,
        _ if c.kind() == Kind::Number && c.tok.is(crate::INTEGER) => {
            let v = c.tok.number(src);
            if v < 1.0 {
                return None;
            }
            // layout caps the tracks a repeat gives; the count is kept as written up to this
            Repeat::Count(v.min(f64::from(u16::MAX)) as u16)
        }
        _ => return None,
    };
    let (es, n) = entries(src, &xs[comma + 1..], true)?;
    // an automatic repeat takes fixed sizes only
    let auto = !matches!(r, Repeat::Count(_));
    let sizes_ok = es.iter().all(|e| match e {
        TrackEntry::Size(s) => !auto || fixed(s),
        _ => true,
    });
    (n > 0 && sizes_ok).then_some((r, (es, n)))
}

/// `grid-template-rows` and `grid-template-columns`.
pub(crate) fn template(src: &str, xs: &[Item]) -> Option<Specified> {
    if let [x] = xs
        && x.ident(src).as_deref() == Some("none")
    {
        return Some(Specified::Tracks(None));
    }
    let (es, _) = entries(src, xs, false)?;
    Some(Specified::Tracks(Some(TrackList(es.into()))))
}

/// `grid-auto-rows` and `grid-auto-columns`.
pub(crate) fn auto_tracks(src: &str, xs: &[Item]) -> Option<Specified> {
    if xs.is_empty() {
        return None;
    }
    let v: Option<Vec<Spec>> = xs.iter().map(|x| track_size(src, x)).collect();
    Some(Specified::AutoTracks(v?.into()))
}

pub(crate) fn auto_flow(src: &str, xs: &[Item]) -> Option<AutoFlow> {
    let mut f = AutoFlow {
        normal: false,
        column: false,
        dense: false,
    };
    let (mut axis, mut dense) = (false, false);
    for x in xs {
        match x.ident(src)?.as_str() {
            "row" if !axis => axis = true,
            "column" if !axis => (axis, f.column) = (true, true),
            "dense" if !dense => (dense, f.dense) = (true, true),
            _ => return None,
        }
    }
    (!xs.is_empty() && xs.len() <= 2).then_some(f)
}

// one cell token of an area string: a name, or `None` for a run of `.`
fn area_cells(s: &str) -> Option<Vec<Option<&str>>> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut k = 0;
    let name = |c: char| c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii();
    while k < b.len() {
        let c = s[k..].chars().next()?;
        if matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r') {
            k += 1;
        } else if c == '.' {
            while k < b.len() && b[k] == b'.' {
                k += 1;
            }
            out.push(None);
        } else if name(c) {
            let start = k;
            while let Some(c) = s[k..].chars().next().filter(|&c| name(c)) {
                k += c.len_utf8();
            }
            out.push(Some(&s[start..k]));
        } else {
            return None;
        }
    }
    Some(out)
}

/// The template areas of the strings, `None` when they are not rectangles of the same width.
pub(crate) fn areas_of(strings: &[String]) -> Option<Areas> {
    let mut named: Vec<(Arc<str>, [u16; 4])> = Vec::new();
    let mut cols = None;
    for (r, s) in strings.iter().enumerate() {
        let cells = area_cells(s)?;
        if cells.is_empty()
            || cells.len() > MAX_TRACKS
            || *cols.get_or_insert(cells.len()) != cells.len()
        {
            return None;
        }
        let r = r as u16;
        let mut c = 0;
        while c < cells.len() {
            let Some(n) = cells[c] else {
                c += 1;
                continue;
            };
            let end = c + cells[c..].iter().take_while(|&&x| x == Some(n)).count();
            let (c0, c1) = (c as u16, end as u16);
            match named.iter_mut().find(|(m, _)| &**m == n) {
                // the row below the area's last, over the same columns
                Some((_, a)) if a[1] == r && a[2] == c0 && a[3] == c1 => a[1] = r + 1,
                Some(_) => return None,
                None => named.push((n.into(), [r, r + 1, c0, c1])),
            }
            c = end;
        }
    }
    if strings.len() > MAX_TRACKS {
        return None;
    }
    Some(Areas {
        rows: strings.len() as u16,
        cols: cols? as u16,
        strings: strings.iter().map(|s| Arc::from(s.as_str())).collect(),
        named: named.into(),
    })
}

/// `grid-template-areas`.
pub(crate) fn areas(src: &str, xs: &[Item]) -> Option<Specified> {
    if let [x] = xs
        && x.ident(src).as_deref() == Some("none")
    {
        return Some(Specified::Areas(None));
    }
    let strings: Option<Vec<String>> = xs
        .iter()
        .map(|x| (x.kind() == Kind::String).then(|| raw(src, x)))
        .collect();
    let a = areas_of(&strings?)?;
    Some(Specified::Areas(Some(Arc::new(a))))
}

/// `grid-row-start` and the like.
pub(crate) fn line(src: &str, xs: &[Item]) -> Option<GridLine> {
    if let [x] = xs {
        if x.ident(src).as_deref() == Some("auto") {
            return Some(GridLine::Auto);
        }
        if let Some(n) = custom_ident(src, x) {
            return Some(GridLine::Ident(n));
        }
    }
    let (mut span, mut n, mut name) = (false, None, None);
    for x in xs {
        if x.ident(src).as_deref() == Some("span") && !span && (n.is_none() && name.is_none()) {
            span = true;
        } else if x.kind() == Kind::Number && x.tok.is(crate::INTEGER) && n.is_none() {
            n = Some(
                x.tok
                    .number(src)
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
            );
        } else {
            name = Some(custom_ident(src, x).filter(|_| name.is_none())?);
        }
    }
    match (span, n) {
        (true, Some(v)) if v > 0 => Some(GridLine::Span(v as u32, name)),
        (true, None) if name.is_some() => Some(GridLine::Span(1, name)),
        (false, Some(v)) if v != 0 => Some(GridLine::Line(v, name)),
        _ => None,
    }
}

/// The values on either side of each `/`.
pub(crate) fn slashed(xs: &[Item]) -> Vec<&[Item]> {
    xs.split(|x| x.is_delim(b'/')).collect()
}

/// The end line a `grid-row`, `grid-column` or `grid-area` leaves out: the start's name if it is one.
pub(crate) fn implied_end(start: &GridLine) -> GridLine {
    match start {
        GridLine::Ident(n) => GridLine::Ident(n.clone()),
        _ => GridLine::Auto,
    }
}
