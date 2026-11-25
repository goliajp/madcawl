//! Intrinsic track sizes from the items' contributions (CSS Grid 11.5): an item in one track sets its
//! base size and growth limit directly; one spanning several spreads what its tracks lack over them, in
//! five phases from the minimums to the maximums, by increasing span; those crossing flexible tracks go to
//! the flexible tracks only.

use crate::au::Au;

use super::tracks::{Avail, Contrib, MaxFn, MinFn, Track, gaps};

/// The phases of distributing a spanning item's contribution (CSS Grid 11.5.3 to 11.5.4).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    IntrinsicMin,
    ContentMin,
    MaxContentMin,
    IntrinsicMax,
    MaxContentMax,
}

impl Phase {
    fn affects(self, t: &Track, avail: Avail) -> bool {
        match self {
            Phase::IntrinsicMin => {
                matches!(t.min, MinFn::MinContent | MinFn::MaxContent | MinFn::Auto)
            }
            Phase::ContentMin => matches!(t.min, MinFn::MinContent | MinFn::MaxContent),
            Phase::MaxContentMin => {
                t.min == MinFn::MaxContent || t.min == MinFn::Auto && avail == Avail::MaxContent
            }
            Phase::IntrinsicMax => t.intrinsic_max(),
            Phase::MaxContentMax => t.max_content_max(),
        }
    }

    fn of(self, c: &Contrib, avail: Avail) -> Au {
        match self {
            Phase::IntrinsicMin if avail == Avail::MinContent => c.min_content,
            Phase::IntrinsicMin => c.minimum,
            Phase::ContentMin | Phase::IntrinsicMax => c.min_content,
            Phase::MaxContentMin | Phase::MaxContentMax => c.max_content,
        }
    }

    fn limits(self) -> bool {
        matches!(self, Phase::IntrinsicMax | Phase::MaxContentMax)
    }
}

const PHASES: [Phase; 5] = [
    Phase::IntrinsicMin,
    Phase::ContentMin,
    Phase::MaxContentMin,
    Phase::IntrinsicMax,
    Phase::MaxContentMax,
];

// a single track spanned (CSS Grid 11.5.2)
pub(super) fn single(t: &mut Track, c: &Contrib, avail: Avail) {
    match t.min {
        MinFn::MinContent => t.base = t.base.max(c.min_content),
        MinFn::MaxContent => t.base = t.base.max(c.max_content),
        MinFn::Auto if avail == Avail::MinContent => t.base = t.base.max(c.min_content),
        MinFn::Auto if avail == Avail::MaxContent => t.base = t.base.max(c.max_content),
        MinFn::Auto => t.base = t.base.max(c.minimum),
        MinFn::Fixed(_) => {}
    }
    let grow = |l: Au, v: Au| if l == Au::MAX { v } else { l.max(v) };
    match t.max {
        MaxFn::MinContent => t.limit = grow(t.limit, c.min_content),
        MaxFn::MaxContent | MaxFn::Auto => t.limit = grow(t.limit, c.max_content),
        MaxFn::Fit(cap) => t.limit = grow(t.limit, c.max_content.min(cap.max(t.base))),
        MaxFn::Fixed(_) | MaxFn::Fr(_) => {}
    }
    if t.limit < t.base {
        t.limit = t.base;
    }
}

// the planned increase of each affected track from spreading `extra` over them (CSS Grid 11.5.1)
fn spread(tracks: &[Track], idx: &[usize], extra: Au, phase: Phase, out: &mut [Au]) {
    if idx.is_empty() || extra <= Au::ZERO {
        return;
    }
    let size = |t: &Track| {
        if phase.limits() {
            t.limit_or_base()
        } else {
            t.base
        }
    };
    let cap = |t: &Track| -> Au {
        match (phase.limits(), t.max) {
            (true, MaxFn::Fit(c)) => (c.max(t.base) - size(t)).max(Au::ZERO),
            (true, _) => Au::MAX,
            (false, _) => {
                if t.limit == Au::MAX {
                    Au::MAX
                } else {
                    (t.limit - t.base).max(Au::ZERO)
                }
            }
        }
    };
    let rooms: Vec<Au> = idx.iter().map(|&i| cap(&tracks[i])).collect();
    let mut inc = vec![Au::ZERO; idx.len()];
    let left = super::tracks::fill_equally(&rooms, extra, &mut inc);
    // past the caps: to the tracks whose maximum is intrinsic (base sizes), or to every one but the
    // fit-content tracks (limits)
    if left > Au::ZERO {
        let to: Vec<usize> = (0..idx.len())
            .filter(|&k| {
                let t = &tracks[idx[k]];
                if phase.limits() {
                    !matches!(t.max, MaxFn::Fit(_))
                } else if phase == Phase::MaxContentMin {
                    t.max_content_max()
                } else {
                    t.intrinsic_max()
                }
            })
            .collect();
        let to: Vec<usize> = if to.is_empty() && !phase.limits() {
            (0..idx.len()).collect()
        } else {
            to
        };
        if !to.is_empty() {
            let share = Au(left.0 / to.len() as i32);
            let rest = left - Au(share.0 * to.len() as i32);
            for (j, &k) in to.iter().enumerate() {
                inc[k] += share + if j == 0 { rest } else { Au::ZERO };
            }
        }
    }
    for (k, &i) in idx.iter().enumerate() {
        out[i] = out[i].max(inc[k]);
    }
}

// items spanning more than one track, none flexible, by increasing span; then those crossing flexible
// tracks, which go to the flexible tracks only
pub(super) fn spanning(
    tracks: &mut [Track],
    items: &[Contrib],
    gap: Au,
    avail: Avail,
    flexed: bool,
) {
    let mut order: Vec<usize> = (0..items.len())
        .filter(|&k| {
            let c = &items[k];
            let crosses = tracks[c.first..c.end].iter().any(|t| t.flex().is_some());
            (c.end - c.first > 1 || flexed) && crosses == flexed
        })
        .collect();
    order.sort_by_key(|&k| items[k].end - items[k].first);
    let mut planned = vec![Au::ZERO; tracks.len()];
    let mut g = 0;
    while g < order.len() {
        let span = items[order[g]].end - items[order[g]].first;
        let group_end = if flexed {
            order.len()
        } else {
            g + order[g..]
                .iter()
                .take_while(|&&k| items[k].end - items[k].first == span)
                .count()
        };
        for phase in PHASES {
            if flexed && phase.limits() {
                continue;
            }
            // only the tracks the group's items span are planned for, then applied and cleared
            let mut touched = Vec::new();
            for &k in &order[g..group_end] {
                let c = &items[k];
                let range = c.first..c.end;
                let idx: Vec<usize> = range
                    .clone()
                    .filter(|&i| {
                        phase.affects(&tracks[i], avail) && (!flexed || tracks[i].flex().is_some())
                    })
                    .collect();
                let have = range.clone().fold(gaps(range.len(), gap), |a, i| {
                    a + if phase.limits() {
                        tracks[i].limit_or_base()
                    } else {
                        tracks[i].base
                    }
                });
                spread(tracks, &idx, phase.of(c, avail) - have, phase, &mut planned);
                touched.extend(idx);
            }
            for i in touched {
                let (t, p) = (&mut tracks[i], std::mem::take(&mut planned[i]));
                if p == Au::ZERO {
                    continue;
                }
                if phase.limits() {
                    t.limit = t.limit_or_base() + p;
                } else {
                    t.base += p;
                    if t.limit < t.base {
                        t.limit = t.base;
                    }
                }
            }
        }
        g = group_end;
    }
}
