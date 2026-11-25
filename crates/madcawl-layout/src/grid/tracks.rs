//! The grid track sizing algorithm for one axis (CSS Grid 11.3 to 11.8): base sizes and growth limits
//! from the sizing functions, intrinsic sizes from the items' contributions, single spans first, then
//! wider spans by increasing span, then spans that cross flexible tracks; tracks maximized into the free
//! space, flexible tracks expanded, and `auto` tracks stretched.

use madcawl_css::style::Lp;
use madcawl_css::style::grid::{Breadth, TrackSize};

use crate::au::Au;
use crate::style::lp;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MinFn {
    Fixed(Au),
    MinContent,
    MaxContent,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaxFn {
    Fixed(Au),
    MinContent,
    MaxContent,
    Auto,
    Fr(f32),
    /// `fit-content()`: max-content, no larger than the argument
    Fit(Au),
}

#[derive(Clone, Copy, Debug)]
pub struct Track {
    pub min: MinFn,
    pub max: MaxFn,
    pub base: Au,
    /// `Au::MAX` is infinite
    pub limit: Au,
    /// An empty track of an `auto-fit` repeat: no size and no gap
    pub collapsed: bool,
}

/// The space an axis is sized in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Avail {
    Definite(Au),
    MinContent,
    MaxContent,
}

/// What an item adds to the tracks it spans, from its margin box.
#[derive(Clone, Copy, Debug)]
pub struct Contrib {
    pub first: usize,
    pub end: usize,
    pub min_content: Au,
    pub max_content: Au,
    /// Its minimum contribution: what its `min-width` or `min-height` gives, or its automatic minimum
    pub minimum: Au,
}

impl Track {
    /// A track of `size`, percentages against `base` when it is definite and `auto` otherwise.
    pub fn new(size: &TrackSize<Lp>, base: Option<Au>) -> Track {
        let len =
            |l: &Lp| (!l.has_pct() || base.is_some()).then(|| lp(*l, base.unwrap_or(Au::ZERO)));
        let min = |b: &Breadth<Lp>| match b {
            Breadth::Len(l) => len(l).map_or(MinFn::Auto, |v| MinFn::Fixed(v.max(Au::ZERO))),
            Breadth::MinContent => MinFn::MinContent,
            Breadth::MaxContent => MinFn::MaxContent,
            Breadth::Auto | Breadth::Fr(_) => MinFn::Auto,
        };
        let max = |b: &Breadth<Lp>| match b {
            Breadth::Len(l) => len(l).map_or(MaxFn::Auto, |v| MaxFn::Fixed(v.max(Au::ZERO))),
            Breadth::MinContent => MaxFn::MinContent,
            Breadth::MaxContent => MaxFn::MaxContent,
            Breadth::Auto => MaxFn::Auto,
            Breadth::Fr(_) => MaxFn::Fr(b.fr().unwrap_or(0.0)),
        };
        let (mn, mx) = match size {
            TrackSize::One(b) => (min(b), max(b)),
            TrackSize::MinMax(a, b) => (min(a), max(b)),
            TrackSize::FitContent(l) => (MinFn::Auto, len(l).map_or(MaxFn::MaxContent, MaxFn::Fit)),
        };
        let base = match mn {
            MinFn::Fixed(v) => v,
            _ => Au::ZERO,
        };
        let limit = match mx {
            MaxFn::Fixed(v) => v.max(base),
            _ => Au::MAX,
        };
        Track {
            min: mn,
            max: mx,
            base,
            limit,
            collapsed: false,
        }
    }

    pub fn collapse() -> Track {
        Track {
            min: MinFn::Fixed(Au::ZERO),
            max: MaxFn::Fixed(Au::ZERO),
            base: Au::ZERO,
            limit: Au::ZERO,
            collapsed: true,
        }
    }

    pub(super) fn flex(&self) -> Option<f32> {
        match self.max {
            MaxFn::Fr(f) => Some(f),
            _ => None,
        }
    }

    pub(super) fn intrinsic_max(&self) -> bool {
        matches!(
            self.max,
            MaxFn::MinContent | MaxFn::MaxContent | MaxFn::Auto | MaxFn::Fit(_)
        )
    }

    pub(super) fn max_content_max(&self) -> bool {
        matches!(self.max, MaxFn::MaxContent | MaxFn::Auto | MaxFn::Fit(_))
    }

    pub(super) fn limit_or_base(&self) -> Au {
        if self.limit == Au::MAX {
            self.base
        } else {
            self.limit
        }
    }
}

pub(super) fn gaps(n: usize, gap: Au) -> Au {
    Au(gap
        .0
        .saturating_mul(n.saturating_sub(1).min(i32::MAX as usize) as i32))
}

/// Sizes `tracks` for `items` in `avail`, `gap` between tracks; `auto` tracks then stretch to fill
/// `stretch`, when the content alignment lets them.
pub fn size(tracks: &mut [Track], items: &[Contrib], gap: Au, avail: Avail, stretch: Option<Au>) {
    for c in items.iter().filter(|c| c.end - c.first == 1) {
        let t = &mut tracks[c.first];
        if t.flex().is_none() {
            super::span::single(t, c, avail);
        }
    }
    super::span::spanning(tracks, items, gap, avail, false);
    super::span::spanning(tracks, items, gap, avail, true);
    for t in tracks.iter_mut() {
        if t.limit == Au::MAX {
            t.limit = t.base;
        }
    }
    let used = |ts: &[Track]| {
        let n = ts.iter().filter(|t| !t.collapsed).count();
        ts.iter().fold(gaps(n, gap), |a, t| a + t.base)
    };
    // maximize: into the free space, or to the growth limits when it is indefinite
    match avail {
        Avail::Definite(v) => {
            let free = v - used(tracks);
            if free > Au::ZERO {
                let idx: Vec<usize> = (0..tracks.len()).collect();
                let mut inc = vec![Au::ZERO; tracks.len()];
                spread_to_limits(tracks, &idx, free, &mut inc);
                for (t, d) in tracks.iter_mut().zip(inc) {
                    t.base += d;
                }
            }
        }
        Avail::MaxContent => tracks.iter_mut().for_each(|t| t.base = t.limit),
        Avail::MinContent => {}
    }
    super::flex::expand(tracks, items, gap, avail);
    if let Some(v) = stretch {
        let free = v - used(tracks);
        let autos: Vec<usize> = (0..tracks.len())
            .filter(|&i| tracks[i].max == MaxFn::Auto && !tracks[i].collapsed)
            .collect();
        if free > Au::ZERO && !autos.is_empty() {
            let share = Au(free.0 / autos.len() as i32);
            for &i in &autos {
                tracks[i].base += share;
            }
        }
    }
}

// equal shares, no track past its growth limit
fn spread_to_limits(tracks: &[Track], idx: &[usize], free: Au, inc: &mut [Au]) {
    let rooms: Vec<Au> = idx
        .iter()
        .map(|&i| (tracks[i].limit - tracks[i].base - inc[i]).max(Au::ZERO))
        .collect();
    let mut add = vec![Au::ZERO; idx.len()];
    fill_equally(&rooms, free, &mut add);
    for (k, &i) in idx.iter().enumerate() {
        inc[i] += add[k];
    }
}

/// Shares `extra` equally, in whole units, among slots of `rooms`, none past its room, adding to `out`;
/// what equal shares leave goes a unit each to the first slots with room. Returns what is left when every
/// slot is full. This is the result of repeated rounds of equal shares, sorted once by room.
pub(super) fn fill_equally(rooms: &[Au], extra: Au, out: &mut [Au]) -> Au {
    if extra <= Au::ZERO {
        return extra.max(Au::ZERO);
    }
    let mut order: Vec<usize> = (0..rooms.len()).filter(|&k| rooms[k] > Au::ZERO).collect();
    order.sort_by_key(|&k| rooms[k]);
    let (mut left, mut level, mut m) = (i64::from(extra.0), 0i64, order.len() as i64);
    let mut full = true;
    for &k in &order {
        let cost = (i64::from(rooms[k].0) - level) * m;
        if cost > left {
            level += left / m;
            left %= m;
            full = false;
            break;
        }
        left -= cost;
        level = i64::from(rooms[k].0);
        m -= 1;
    }
    for (k, &r) in rooms.iter().enumerate() {
        let r = i64::from(r.0);
        let mut d = r.min(level).max(0);
        if !full && left > 0 && r > level {
            d += 1;
            left -= 1;
        }
        out[k] += Au(d as i32);
    }
    if full { Au(left as i32) } else { Au::ZERO }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(b: Breadth<Lp>) -> Track {
        Track::new(&TrackSize::One(b), Some(Au::from_px(1000.0)))
    }

    fn fr(v: f32) -> Breadth<Lp> {
        Breadth::Fr(v.to_bits())
    }

    fn item(first: usize, end: usize, min: f32, max: f32) -> Contrib {
        Contrib {
            first,
            end,
            min_content: Au::from_px(min),
            max_content: Au::from_px(max),
            minimum: Au::from_px(min),
        }
    }

    fn bases(ts: &[Track]) -> Vec<f32> {
        ts.iter().map(|t| t.base.px()).collect()
    }

    #[test]
    fn flexible_tracks_share_the_free_space_and_keep_their_items_minimum() {
        let mut ts = [track(fr(1.0)), track(fr(1.0))];
        size(
            &mut ts,
            &[item(0, 1, 300.0, 300.0)],
            Au::ZERO,
            Avail::Definite(Au::from_px(400.0)),
            None,
        );
        assert_eq!(bases(&ts), [300.0, 100.0]);
        // shares that do not divide evenly add up to the space, the last taking the rest
        let mut ts = [track(fr(1.0)), track(fr(1.0)), track(fr(1.0))];
        size(&mut ts, &[], Au::ZERO, Avail::Definite(Au(30976)), None);
        assert_eq!(ts.map(|t| t.base.0), [10325, 10325, 10326]);
    }

    #[test]
    fn a_spanning_item_spreads_what_its_tracks_lack_over_the_ones_that_can_grow() {
        let px = |v| Breadth::Len(Lp::px(v));
        let mut ts = [track(Breadth::Auto), track(Breadth::Auto), track(px(50.0))];
        let items = [item(0, 1, 20.0, 20.0), item(0, 3, 300.0, 300.0)];
        size(&mut ts, &items, Au::ZERO, Avail::MinContent, None);
        // the first track's growth limit stops it at its own item; the fixed one does not grow
        assert_eq!(bases(&ts), [20.0, 230.0, 50.0]);
    }

    #[test]
    fn under_a_max_content_constraint_tracks_take_their_limits_and_fr_its_largest_need() {
        let mut ts = [track(Breadth::Auto), track(fr(1.0)), track(fr(2.0))];
        let items = [
            item(0, 1, 10.0, 40.0),
            item(1, 2, 30.0, 30.0),
            item(2, 3, 30.0, 30.0),
        ];
        size(&mut ts, &items, Au::ZERO, Avail::MaxContent, None);
        assert_eq!(bases(&ts), [40.0, 30.0, 60.0]);
    }

    #[test]
    fn auto_tracks_stretch_into_what_is_left() {
        let px = |v| Breadth::Len(Lp::px(v));
        let mut ts = [track(Breadth::Auto), track(px(100.0)), track(Breadth::Auto)];
        let items = [item(0, 1, 10.0, 50.0), item(2, 3, 10.0, 10.0)];
        let avail = Au::from_px(300.0);
        size(
            &mut ts,
            &items,
            Au::from_px(10.0),
            Avail::Definite(avail),
            Some(avail),
        );
        assert_eq!(bases(&ts), [110.0, 100.0, 70.0]);
    }

    // repeated rounds of equal shares, as 11.5.1 states them
    fn rounds(rooms: &[Au], extra: Au) -> (Vec<Au>, Au) {
        let (mut inc, mut left) = (vec![Au::ZERO; rooms.len()], extra);
        for _ in 0..rooms.len() {
            let open: Vec<usize> = (0..rooms.len()).filter(|&k| inc[k] < rooms[k]).collect();
            if open.is_empty() || left <= Au::ZERO {
                break;
            }
            let share = Au((left.0 / open.len() as i32).max(1));
            for &k in &open {
                let d = share.min(rooms[k] - inc[k]).min(left);
                inc[k] += d;
                left -= d;
            }
        }
        let full = (0..rooms.len()).all(|k| inc[k] >= rooms[k]);
        (inc, if full { left } else { Au::ZERO })
    }

    #[test]
    fn one_pass_equal_shares_give_what_repeated_rounds_give() {
        let mut seed = 0x2545_f491_u32;
        let mut next = |m: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % m
        };
        for _ in 0..2000 {
            let n = 1 + next(7) as usize;
            let rooms: Vec<Au> = (0..n)
                .map(|_| match next(5) {
                    0 => Au::MAX,
                    1 => Au::ZERO,
                    _ => Au(next(400) as i32),
                })
                .collect();
            let extra = Au(next(1500) as i32);
            let mut out = vec![Au::ZERO; n];
            let left = fill_equally(&rooms, extra, &mut out);
            let (want, want_left) = rounds(&rooms, extra);
            assert_eq!((&out, left), (&want, want_left), "{rooms:?} {extra:?}");
        }
    }
}
