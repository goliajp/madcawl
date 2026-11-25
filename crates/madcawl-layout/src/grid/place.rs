//! Placing grid items (CSS Grid 8): line-based and area placement resolved against the explicit grid, then
//! the auto-placement algorithm, sparse or dense, by rows or by columns. Lines are counted from the
//! explicit grid's first, and may be negative before it.

use madcawl_css::style::grid::GridLine;

use super::explicit::{Explicit, MAX_LINES};

/// An item's lines on one axis, or its span when auto-placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pos {
    Lines(i32, i32),
    Auto(i32),
}

// the nth line named `name` after (`up`) or before `from`; lines past the explicit grid all have the name
fn nth_named(ax: &Explicit, name: &str, n: i32, from: i32, up: bool) -> i32 {
    let n = n.max(1);
    let found: Vec<i32> = if up {
        ax.lines(name)
            .filter(|&l| l > from)
            .take(n as usize)
            .collect()
    } else {
        ax.lines(name)
            .rev()
            .filter(|&l| l < from)
            .take(n as usize)
            .collect()
    };
    if found.len() as i32 == n {
        return *found.last().expect("n is at least 1");
    }
    let left = n - found.len() as i32;
    if up {
        from.max(ax.tracks()) + left
    } else {
        from.min(0) - left
    }
}

// a definite line of `g`, or `None` for `auto` and a span
fn line(ax: &Explicit, g: &GridLine, end: bool) -> Option<i32> {
    let n = ax.tracks();
    Some(match g {
        GridLine::Auto | GridLine::Span(..) => return None,
        GridLine::Line(k, None) if *k > 0 => k - 1,
        GridLine::Line(k, None) => n.saturating_add(1).saturating_add(*k),
        GridLine::Line(k, Some(name)) if *k > 0 => {
            nth_named(ax, name, (*k).min(MAX_LINES), -1, true)
        }
        GridLine::Line(k, Some(name)) => {
            nth_named(ax, name, k.saturating_neg().min(MAX_LINES), n + 1, false)
        }
        GridLine::Ident(name) => {
            let edge = format!("{name}-{}", if end { "end" } else { "start" });
            match ax.lines(&edge).next() {
                Some(l) => l,
                None => nth_named(ax, name, 1, -1, true),
            }
        }
    })
}

fn clamp(l: i32) -> i32 {
    l.clamp(-MAX_LINES, MAX_LINES)
}

/// The lines of an item on an axis from its start and end (CSS Grid 8.3.1).
pub fn resolve(ax: &Explicit, start: &GridLine, end: &GridLine) -> Pos {
    let span = |g: &GridLine| match g {
        GridLine::Span(k, n) => Some(((*k).min(MAX_LINES as u32) as i32, n.clone())),
        _ => None,
    };
    let (s, e) = (line(ax, start, false), line(ax, end, true));
    let (s, e) = match (s, e) {
        (Some(s), Some(e)) if e < s => (s.min(e), s.max(e)),
        (Some(s), Some(e)) if e == s => (s, s + 1),
        (Some(s), Some(e)) => (s, e),
        (Some(s), None) => match span(end) {
            Some((k, Some(name))) => (s, nth_named(ax, &name, k, s, true)),
            Some((k, None)) => (s, s.saturating_add(k)),
            None => (s, s + 1),
        },
        (None, Some(e)) => match span(start) {
            Some((k, Some(name))) => (nth_named(ax, &name, k, e, false), e),
            Some((k, None)) => (e.saturating_sub(k), e),
            None => (e - 1, e),
        },
        (None, None) => {
            // a span on the end only counts when the start does not have one
            let k = span(start)
                .or_else(|| span(end))
                .map_or(1, |(k, n)| if n.is_some() { 1 } else { k });
            return Pos::Auto(k.max(1));
        }
    };
    Pos::Lines(clamp(s), clamp(e).max(clamp(s) + 1))
}

/// The areas items take, `[down start, down end, across start, across end]`, one per placed item; `work`
/// bounds the area checks one layout makes, past which items are placed without looking at what they overlap.
struct Taken {
    areas: Vec<[i32; 4]>,
    work: u32,
}

impl Taken {
    /// Where across the areas in the way end, the next place to try; the line down the flow where the area
    /// ending there ends, until which every place before it across stays blocked; and the line below all of
    /// them. `None` when nothing is in the way.
    fn blocked(&mut self, down: (i32, i32), across: (i32, i32)) -> Option<Blocked> {
        let n = (self.areas.len() as u32).max(1);
        if self.work < n {
            self.work = 0;
            return None;
        }
        self.work -= n;
        let mut out: Option<Blocked> = None;
        for &[d0, d1, a0, a1] in &self.areas {
            if d1 > down.0 && d0 < down.1 && a1 > across.0 && a0 < across.1 {
                let o = out.get_or_insert(Blocked {
                    across: a1,
                    until: d1,
                    below: d1,
                });
                if (a1, d1) > (o.across, o.until) {
                    (o.across, o.until) = (a1, d1);
                }
                o.below = o.below.max(d1);
            }
        }
        out
    }

    fn take(&mut self, down: (i32, i32), across: (i32, i32)) {
        self.areas.push([down.0, down.1, across.0, across.1]);
    }
}

struct Blocked {
    across: i32,
    until: i32,
    below: i32,
}

// the area checks one container's auto-placement makes, each costing the areas placed so far
const WORK: u32 = 1 << 24;

/// One item: its position down the flow (rows for a row flow) and across it, filled in by `auto_place`.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub down: Pos,
    pub across: Pos,
}

/// The auto-placement algorithm (CSS Grid 8.5) over `items` in order-modified document order, the
/// explicit grid having `explicit_across` tracks across the flow; returns the first and last lines across
/// the implicit grid.
pub fn auto_place(items: &mut [Item], dense: bool, explicit_across: i32) -> (i32, i32) {
    let mut taken = Taken {
        areas: Vec::with_capacity(items.len()),
        work: WORK,
    };
    for it in items.iter() {
        if let (Pos::Lines(d0, d1), Pos::Lines(a0, a1)) = (it.down, it.across) {
            taken.take((d0, d1), (a0, a1));
        }
    }
    // items locked to a line down the flow
    let mut cursor: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
    for it in items.iter_mut() {
        let (Pos::Lines(d0, d1), Pos::Auto(k)) = (it.down, it.across) else {
            continue;
        };
        let mut a = if dense {
            0
        } else {
            *cursor.get(&d0).unwrap_or(&0)
        };
        while let Some(Blocked { across: next, .. }) = taken.blocked((d0, d1), (a, a + k)) {
            if a >= MAX_LINES {
                break;
            }
            a = next.max(a + 1);
        }
        it.across = Pos::Lines(a, a + k);
        cursor.insert(d0, a + k);
        taken.take((d0, d1), (a, a + k));
    }
    // the lines across the implicit grid
    let (mut lo, mut hi) = (0, explicit_across.max(0));
    for it in items.iter() {
        match it.across {
            Pos::Lines(a, b) => (lo, hi) = (lo.min(a), hi.max(b)),
            Pos::Auto(k) => hi = hi.max(lo + k),
        }
    }
    let lo_down = items
        .iter()
        .filter_map(|it| match it.down {
            Pos::Lines(d, _) => Some(d),
            _ => None,
        })
        .min()
        .unwrap_or(0)
        .min(0);
    let (mut cd, mut ca) = (lo_down, lo);
    for it in items.iter_mut() {
        let Pos::Auto(dk) = it.down else { continue };
        match it.across {
            Pos::Lines(a0, a1) => {
                if dense {
                    cd = lo_down;
                } else if a0 < ca {
                    cd += 1;
                }
                // an area in the way stays in the way until the item starts below it
                while let Some(Blocked { below, .. }) = taken.blocked((cd, cd + dk), (a0, a1)) {
                    cd = below.max(cd + 1);
                }
                ca = a0;
                it.down = Pos::Lines(cd, cd + dk);
                taken.take((cd, cd + dk), (a0, a1));
            }
            Pos::Auto(ak) => {
                if dense {
                    (cd, ca) = (lo_down, lo);
                }
                // every place tried in this row stays blocked down to the first `until`
                let mut row_free = i32::MAX;
                loop {
                    if ca + ak > hi {
                        let next = if row_free == i32::MAX { cd + 1 } else { row_free };
                        (cd, ca, row_free) = (next.max(cd + 1), lo, i32::MAX);
                        continue;
                    }
                    match taken.blocked((cd, cd + dk), (ca, ca + ak)) {
                        None => break,
                        Some(Blocked { across, until, .. }) => {
                            ca = across.max(ca + 1);
                            row_free = row_free.min(until);
                        }
                    }
                }
                it.down = Pos::Lines(cd, cd + dk);
                it.across = Pos::Lines(ca, ca + ak);
                taken.take((cd, cd + dk), (ca, ca + ak));
                ca += ak;
            }
        }
    }
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use madcawl_css::style::grid::{Breadth, TrackSize};

    use super::*;

    fn axis() -> Explicit {
        let auto = TrackSize::One(Breadth::Auto);
        Explicit {
            sizes: vec![auto.clone(); 3],
            names: vec![("a".into(), 0), ("b".into(), 1), ("a".into(), 2)],
            auto: [auto].into(),
            fit: None,
        }
    }

    fn named(n: &str) -> Option<Arc<str>> {
        Some(n.into())
    }

    #[test]
    fn lines_by_number_name_and_span_resolve_against_the_explicit_grid() {
        use GridLine::*;
        let ax = axis();
        let cases = [
            (Line(2, None), Auto, Pos::Lines(1, 2)),
            (Line(-1, None), Auto, Pos::Lines(3, 4)),
            (Line(-5, None), Auto, Pos::Lines(-1, 0)),
            (Ident("a".into()), Auto, Pos::Lines(0, 1)),
            (Line(2, named("a")), Auto, Pos::Lines(2, 3)),
            // past the named lines there are, every implicit line has the name
            (Line(3, named("a")), Auto, Pos::Lines(4, 5)),
            (Line(1, None), Span(2, None), Pos::Lines(0, 2)),
            (Line(4, None), Line(1, None), Pos::Lines(0, 3)),
            (Span(1, named("a")), Line(3, None), Pos::Lines(0, 2)),
            (Line(2, None), Line(2, None), Pos::Lines(1, 2)),
            (Span(3, None), Auto, Pos::Auto(3)),
            (Auto, Span(2, None), Pos::Auto(2)),
        ];
        for (s, e, want) in cases {
            assert_eq!(resolve(&ax, &s, &e), want, "{s:?} / {e:?}");
        }
    }

    #[test]
    fn lines_past_the_cap_clamp_to_it() {
        let ax = axis();
        let far = resolve(&ax, &GridLine::Line(i32::MAX, None), &GridLine::Auto);
        assert_eq!(far, Pos::Lines(MAX_LINES, MAX_LINES + 1));
        let before = resolve(&ax, &GridLine::Line(i32::MIN, None), &GridLine::Auto);
        assert_eq!(before, Pos::Lines(-MAX_LINES, -MAX_LINES + 1));
        let wide = resolve(
            &ax,
            &GridLine::Line(1, None),
            &GridLine::Span(u32::MAX, None),
        );
        assert_eq!(wide, Pos::Lines(0, MAX_LINES));
    }

    #[test]
    fn auto_placement_fills_earlier_holes_only_when_dense() {
        let items = || {
            [2, 2, 1].map(|k| Item {
                down: Pos::Auto(1),
                across: Pos::Auto(k),
            })
        };
        let mut sparse = items();
        auto_place(&mut sparse, false, 3);
        let at: Vec<_> = sparse.iter().map(|i| (i.down, i.across)).collect();
        assert_eq!(
            at,
            [
                (Pos::Lines(0, 1), Pos::Lines(0, 2)),
                (Pos::Lines(1, 2), Pos::Lines(0, 2)),
                (Pos::Lines(1, 2), Pos::Lines(2, 3)),
            ]
        );
        let mut dense = items();
        auto_place(&mut dense, true, 3);
        assert_eq!(
            (dense[2].down, dense[2].across),
            (Pos::Lines(0, 1), Pos::Lines(2, 3))
        );
    }

    #[test]
    fn items_locked_to_a_row_go_first_and_others_flow_around_them() {
        let mut items = [
            Item {
                down: Pos::Auto(1),
                across: Pos::Auto(1),
            },
            Item {
                down: Pos::Lines(0, 1),
                across: Pos::Auto(1),
            },
            Item {
                down: Pos::Auto(1),
                across: Pos::Lines(0, 1),
            },
        ];
        auto_place(&mut items, false, 2);
        assert_eq!(items[1].across, Pos::Lines(0, 1));
        assert_eq!(
            (items[0].down, items[0].across),
            (Pos::Lines(0, 1), Pos::Lines(1, 2))
        );
        assert_eq!(items[2].down, Pos::Lines(1, 2));
    }

    #[test]
    fn items_spanning_the_whole_axis_are_placed_by_jumping_past_what_is_in_the_way() {
        let mut items = [0; 200].map(|_| Item {
            down: Pos::Auto(MAX_LINES),
            across: Pos::Auto(1),
        });
        auto_place(&mut items, false, 1);
        for (k, it) in items.iter().enumerate() {
            let d = k as i32 * MAX_LINES;
            assert_eq!((it.down, it.across), (Pos::Lines(d, d + MAX_LINES), Pos::Lines(0, 1)));
        }
        let mut fixed = [0; 200].map(|_| Item {
            down: Pos::Auto(MAX_LINES),
            across: Pos::Lines(0, 1),
        });
        auto_place(&mut fixed, false, 1);
        assert_eq!(fixed[199].down, Pos::Lines(199 * MAX_LINES, 200 * MAX_LINES));
    }
}
