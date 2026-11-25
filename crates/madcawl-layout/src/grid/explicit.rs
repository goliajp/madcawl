//! The explicit grid of one axis (CSS Grid 7): its track list expanded, `repeat()` with its count, the
//! line names, those the template areas imply, and the sizes of the implicit tracks around it.

use std::sync::Arc;

use madcawl_css::style::Lp;
use madcawl_css::style::grid::{Areas, Repeat, TrackEntry, TrackList, TrackSize};

/// The most lines a grid's axis has: positions past it are clamped, and so are the tracks a template or
/// `repeat()` gives. WebKit allows a million; ten thousand keeps a container's tracks to a few hundred KB.
pub const MAX_LINES: i32 = 10_000;

pub struct Explicit {
    pub sizes: Vec<TrackSize<Lp>>,
    /// Each named line and its index, in line order
    pub names: Vec<(Arc<str>, i32)>,
    pub auto: Arc<[TrackSize<Lp>]>,
    /// The tracks an `auto-fit` repeat gave, which collapse when empty
    pub fit: Option<(usize, usize)>,
}

impl Explicit {
    pub fn tracks(&self) -> i32 {
        self.sizes.len() as i32
    }

    /// The size of track `i`, counted from the explicit grid's first; implicit tracks repeat the auto
    /// track list forward from the end and backward from the start.
    pub fn size(&self, i: i32) -> &TrackSize<Lp> {
        let n = self.tracks();
        if (0..n).contains(&i) {
            return &self.sizes[i as usize];
        }
        let len = self.auto.len() as i32;
        let k = if i >= n {
            (i - n) % len
        } else {
            len - 1 - ((-i - 1) % len)
        };
        &self.auto[k as usize]
    }

    /// The lines named `name`, in order.
    pub fn lines<'a>(&'a self, name: &'a str) -> impl DoubleEndedIterator<Item = i32> + 'a {
        self.names
            .iter()
            .filter(move |(n, _)| &**n == name)
            .map(|&(_, l)| l)
    }
}

/// How many times an automatic repeat repeats in `avail` (CSS Grid 7.2.3.2): as many as fit, at least one,
/// each track taken at its fixed maximum, or its fixed minimum when the maximum is not fixed.
pub fn auto_repeat_count(list: &TrackList<Lp>, avail: Option<i32>, gap: i32) -> usize {
    let Some(avail) = avail else { return 1 };
    let fixed = |s: &TrackSize<Lp>| -> i32 {
        use madcawl_css::style::grid::Breadth as B;
        let len = |b: &B<Lp>| match b {
            B::Len(l) => Some(crate::style::lp(*l, crate::au::Au(avail)).0),
            _ => None,
        };
        match s {
            TrackSize::One(b) => len(b).unwrap_or(0),
            TrackSize::MinMax(a, b) => len(b).or_else(|| len(a)).unwrap_or(0),
            TrackSize::FitContent(l) => len(&B::Len(*l)).unwrap_or(0),
        }
        .max(0)
    };
    let (mut other, mut other_n, mut rep, mut rep_n) = (0i64, 0i64, 0i64, 0i64);
    for e in list.0.iter() {
        match e {
            TrackEntry::Size(s) => (other, other_n) = (other + i64::from(fixed(s)), other_n + 1),
            TrackEntry::Repeat(Repeat::AutoFill | Repeat::AutoFit, inner) => {
                for s in inner.iter() {
                    if let TrackEntry::Size(s) = s {
                        (rep, rep_n) = (rep + i64::from(fixed(s)), rep_n + 1);
                    }
                }
            }
            TrackEntry::Repeat(Repeat::Count(c), inner) => {
                let k = i64::from(*c);
                for s in inner.iter() {
                    if let TrackEntry::Size(s) = s {
                        (other, other_n) = (other + k * i64::from(fixed(s)), other_n + k);
                    }
                }
            }
            TrackEntry::Names(_) => {}
        }
    }
    let gap = i64::from(gap.max(0));
    let step = rep + rep_n * gap;
    if step <= 0 {
        return 1;
    }
    // the tracks outside the repeat, with a gap after each
    let left = i64::from(avail) - other - other_n * gap + gap;
    ((left / step).max(1) as usize).min(MAX_LINES as usize)
}

fn push_names(names: &mut Vec<(Arc<str>, i32)>, ns: &[Arc<str>], line: i32) {
    names.extend(ns.iter().map(|n| (n.clone(), line)));
}

/// The explicit grid of an axis from its template, the areas (`rows` says which of their axes), and the
/// count an automatic repeat takes.
pub fn explicit(
    list: Option<&TrackList<Lp>>,
    areas: Option<&Areas>,
    rows: bool,
    auto: Arc<[TrackSize<Lp>]>,
    repeats: usize,
) -> Explicit {
    let (mut sizes, mut names, mut fit) = (Vec::new(), Vec::new(), None);
    let cap = MAX_LINES as usize - 1;
    for e in list.map_or(&[][..], |l| &l.0[..]) {
        match e {
            TrackEntry::Names(ns) => push_names(&mut names, ns, sizes.len() as i32),
            TrackEntry::Size(s) if sizes.len() < cap => sizes.push(s.clone()),
            TrackEntry::Size(_) => {}
            TrackEntry::Repeat(r, inner) => {
                let n = match r {
                    Repeat::Count(c) => usize::from(*c),
                    _ => repeats,
                };
                let start = sizes.len();
                for _ in 0..n {
                    if sizes.len() >= cap {
                        break;
                    }
                    for x in inner.iter() {
                        match x {
                            TrackEntry::Names(ns) => push_names(&mut names, ns, sizes.len() as i32),
                            TrackEntry::Size(s) if sizes.len() < cap => sizes.push(s.clone()),
                            _ => {}
                        }
                    }
                }
                if *r == Repeat::AutoFit {
                    fit = Some((start, sizes.len()));
                }
            }
        }
    }
    if let Some(a) = areas {
        let n = usize::from(if rows { a.rows } else { a.cols });
        let len = auto.len();
        let mut k = 0;
        while sizes.len() < n {
            sizes.push(auto[k % len].clone());
            k += 1;
        }
        for (name, l) in a.named.iter() {
            let (s, e) = if rows { (l[0], l[1]) } else { (l[2], l[3]) };
            names.push((format!("{name}-start").into(), i32::from(s)));
            names.push((format!("{name}-end").into(), i32::from(e)));
        }
    }
    names.sort_by_key(|&(_, l)| l);
    Explicit {
        sizes,
        names,
        auto,
        fit,
    }
}

#[cfg(test)]
mod tests {
    use madcawl_css::style::grid::Breadth;

    use super::*;

    fn list(es: Vec<TrackEntry<Lp>>) -> TrackList<Lp> {
        TrackList(es.into())
    }

    fn px(v: f32) -> Breadth<Lp> {
        Breadth::Len(Lp::px(v))
    }

    #[test]
    fn an_automatic_repeat_repeats_as_often_as_its_tracks_fit() {
        let inner: Arc<[TrackEntry<Lp>]> = [TrackEntry::Size(TrackSize::MinMax(
            px(120.0),
            Breadth::Fr(1.0f32.to_bits()),
        ))]
        .into();
        let l = list(vec![TrackEntry::Repeat(Repeat::AutoFill, inner.clone())]);
        let at = |w: f32, g: f32| auto_repeat_count(&l, Some((w * 64.0) as i32), (g * 64.0) as i32);
        assert_eq!(at(500.0, 8.0), 3);
        assert_eq!(at(504.0, 8.0), 4);
        assert_eq!(at(50.0, 0.0), 1, "at least once");
        assert_eq!(
            auto_repeat_count(&l, None, 0),
            1,
            "once when the space is not definite"
        );
        let with_fixed = list(vec![
            TrackEntry::Size(TrackSize::One(px(100.0))),
            TrackEntry::Repeat(Repeat::AutoFit, inner),
        ]);
        assert_eq!(auto_repeat_count(&with_fixed, Some(500 * 64), 0), 3);
    }

    #[test]
    fn templates_expand_repeats_names_and_areas_and_implicit_tracks_cycle_the_auto_list() {
        let n: Arc<[Arc<str>]> = ["x".into()].into();
        let l = list(vec![
            TrackEntry::Names(n.clone()),
            TrackEntry::Repeat(
                Repeat::Count(2),
                [
                    TrackEntry::Size(TrackSize::One(px(10.0))),
                    TrackEntry::Names(n),
                ]
                .into(),
            ),
        ]);
        let auto: Arc<[TrackSize<Lp>]> = [TrackSize::One(px(1.0)), TrackSize::One(px(2.0))].into();
        let ax = explicit(Some(&l), None, false, auto, 1);
        assert_eq!(ax.tracks(), 2);
        assert_eq!(ax.lines("x").collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(ax.size(2), &TrackSize::One(px(1.0)));
        assert_eq!(ax.size(3), &TrackSize::One(px(2.0)));
        assert_eq!(ax.size(-1), &TrackSize::One(px(2.0)));
        assert_eq!(ax.size(-2), &TrackSize::One(px(1.0)));
        let huge = list(vec![TrackEntry::Repeat(
            Repeat::Count(u16::MAX),
            [TrackEntry::Size(TrackSize::One(px(1.0)))].into(),
        )]);
        let ax = explicit(
            Some(&huge),
            None,
            false,
            [TrackSize::One(Breadth::Auto)].into(),
            1,
        );
        assert_eq!(ax.tracks(), MAX_LINES - 1);
    }
}
