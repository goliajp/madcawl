//! `grid-template` and `grid`, and the line shorthands `grid-row`, `grid-column` and `grid-area`.

use std::sync::Arc;

use crate::tokenizer::Kind;

use super::super::grid::{AutoFlow, Breadth, TrackEntry, TrackList, TrackSize};
use super::super::grid_parse::{
    areas_of, auto_tracks, implied_end, line, names, slashed, template, track_size,
};
use super::super::props::Specified;
use super::super::vals::Item;

fn raw(src: &str, x: &Item) -> String {
    let mut s = String::new();
    x.tok.write_value(src, &mut s);
    s
}

/// `grid-row` and `grid-column`: start, end.
pub(super) fn line_pair(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    let parts = slashed(xs);
    let start = line(src, parts.first()?)?;
    let end = match parts.get(1) {
        Some(p) => line(src, p)?,
        None => implied_end(&start),
    };
    (parts.len() <= 2).then(|| vec![Specified::GridLine(start), Specified::GridLine(end)])
}

/// `grid-area`: row start, column start, row end, column end.
pub(super) fn area(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    let parts = slashed(xs);
    if parts.len() > 4 {
        return None;
    }
    let mut v: Vec<_> = parts.iter().map(|p| line(src, p)).collect::<Option<_>>()?;
    // a missing column start copies the row start's name; a missing end its start's
    if v.len() < 2 {
        v.push(implied_end(&v[0]));
    }
    if v.len() < 3 {
        v.push(implied_end(&v[0]));
    }
    if v.len() < 4 {
        v.push(implied_end(&v[1]));
    }
    Some(v.into_iter().map(Specified::GridLine).collect())
}

// the strings form of `grid-template`: rows with their names and sizes, the areas, and the columns
fn with_areas(src: &str, xs: &[Item]) -> Option<[Specified; 3]> {
    let parts = slashed(xs);
    let (rows_part, cols) = match &parts[..] {
        [r] => (*r, Specified::Tracks(None)),
        [r, c] => {
            let Specified::Tracks(Some(t)) = template(src, c)? else {
                return None;
            };
            let auto = t.0.iter().any(|e| matches!(e, TrackEntry::Repeat(..)));
            (*r, (!auto).then_some(Specified::Tracks(Some(t)))?)
        }
        _ => return None,
    };
    let (mut rows, mut strings) = (Vec::new(), Vec::new());
    let mut k = 0;
    while k < rows_part.len() {
        let mut before = None;
        if let Some(n) = names(src, &rows_part[k]) {
            before = Some(n);
            k += 1;
        }
        let s = rows_part.get(k).filter(|x| x.kind() == Kind::String)?;
        strings.push(raw(src, s));
        k += 1;
        let mut size = TrackSize::One(Breadth::Auto);
        if let Some(x) = rows_part.get(k).filter(|x| x.kind() != Kind::String)
            && let Some(t) = track_size(src, x)
        {
            size = t;
            k += 1;
        }
        let after = rows_part.get(k).and_then(|x| names(src, x));
        k += usize::from(after.is_some());
        match (rows.last_mut(), before) {
            // the names after one row and before the next name the same line
            (Some(TrackEntry::Names(prev)), Some(b)) => {
                let merged: Vec<Arc<str>> = prev.iter().chain(b.iter()).cloned().collect();
                *prev = merged.into();
            }
            (_, Some(b)) => rows.push(TrackEntry::Names(b)),
            _ => {}
        }
        rows.push(TrackEntry::Size(size));
        if let Some(a) = after {
            rows.push(TrackEntry::Names(a));
        }
    }
    let areas = areas_of(&strings)?;
    Some([
        Specified::Tracks(Some(TrackList(rows.into()))),
        cols,
        Specified::Areas(Some(Arc::new(areas))),
    ])
}

/// `grid-template`: rows, columns, areas.
pub(super) fn grid_template(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    if let [x] = xs
        && x.ident(src).as_deref() == Some("none")
    {
        return Some(vec![
            Specified::Tracks(None),
            Specified::Tracks(None),
            Specified::Areas(None),
        ]);
    }
    if xs.iter().any(|x| x.kind() == Kind::String) {
        return Some(with_areas(src, xs)?.into());
    }
    let parts = slashed(xs);
    let [r, c] = &parts[..] else { return None };
    Some(vec![
        template(src, r)?,
        template(src, c)?,
        Specified::Areas(None),
    ])
}

fn auto_tracks_or_initial(src: &str, xs: &[Item]) -> Option<Specified> {
    if xs.is_empty() {
        return Some(Specified::AutoTracks(
            [TrackSize::One(Breadth::Auto)].into(),
        ));
    }
    auto_tracks(src, xs)
}

// `auto-flow` with or without `dense`, then the rest
fn flow_prefix<'a>(src: &str, xs: &'a [Item], column: bool) -> Option<(AutoFlow, &'a [Item])> {
    let n = xs
        .iter()
        .take(2)
        .take_while(|x| matches!(x.ident(src).as_deref(), Some("auto-flow" | "dense")))
        .count();
    let words: Vec<String> = xs[..n].iter().filter_map(|x| x.ident(src)).collect();
    if !words.iter().any(|w| w == "auto-flow") || words.iter().filter(|w| *w == "dense").count() > 1
    {
        return None;
    }
    let f = AutoFlow {
        normal: false,
        column,
        dense: words.iter().any(|w| w == "dense"),
    };
    Some((f, &xs[n..]))
}

/// `grid`: template rows, columns, areas, auto rows, auto columns, auto flow.
pub(super) fn grid(src: &str, xs: &[Item]) -> Option<Vec<Specified>> {
    let initial_auto = || Specified::AutoTracks([TrackSize::One(Breadth::Auto)].into());
    if let Some(mut v) = grid_template(src, xs) {
        v.extend([
            initial_auto(),
            initial_auto(),
            Specified::AutoFlow(AutoFlow::NORMAL),
        ]);
        return Some(v);
    }
    let parts = slashed(xs);
    let [l, r] = &parts[..] else { return None };
    if let Some((f, rest)) = flow_prefix(src, r, true) {
        return Some(vec![
            template(src, l)?,
            Specified::Tracks(None),
            Specified::Areas(None),
            initial_auto(),
            auto_tracks_or_initial(src, rest)?,
            Specified::AutoFlow(f),
        ]);
    }
    let (f, rest) = flow_prefix(src, l, false)?;
    Some(vec![
        Specified::Tracks(None),
        template(src, r)?,
        Specified::Areas(None),
        auto_tracks_or_initial(src, rest)?,
        initial_auto(),
        Specified::AutoFlow(f),
    ])
}
