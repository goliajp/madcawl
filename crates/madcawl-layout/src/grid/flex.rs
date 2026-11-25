//! Expanding flexible tracks (CSS Grid 11.7): the used size of `1fr`, from the free space when it is
//! definite, else from the tracks' base sizes and the max-content contributions of the items crossing them.

use crate::au::Au;

use super::tracks::{Avail, Contrib, MaxFn, Track};

fn flex(t: &Track) -> Option<f32> {
    match t.max {
        MaxFn::Fr(f) => Some(f),
        _ => None,
    }
}

// CSS Grid 11.7.1: the size of one fr filling `space` with the tracks in `range`
// in units of 1/64 px
fn find_fr(tracks: &[Track], range: std::ops::Range<usize>, space: Au) -> f64 {
    let f = |t: &Track| f64::from(flex(t).unwrap_or(0.0));
    let (mut left, mut sum) = (f64::from(space.0), 0.0f64);
    let mut flexible: Vec<usize> = Vec::new();
    for i in range {
        match flex(&tracks[i]) {
            Some(_) => {
                sum += f(&tracks[i]);
                flexible.push(i);
            }
            None => left -= f64::from(tracks[i].base.0),
        }
    }
    // a track whose base is more than its share of an fr is inflexible; taking the largest first lowers
    // the fr each time, as the repeated passes of 11.7.1 do
    let ratio = |i: usize| f64::from(tracks[i].base.0) / f(&tracks[i]).max(f64::MIN_POSITIVE);
    flexible.sort_by(|&a, &b| ratio(b).total_cmp(&ratio(a)));
    for &i in &flexible {
        let fr = left.max(0.0) / sum.max(1.0);
        if f64::from(tracks[i].base.0) <= fr * f(&tracks[i]) {
            return fr;
        }
        left -= f64::from(tracks[i].base.0);
        sum -= f(&tracks[i]);
    }
    left.max(0.0) / sum.max(1.0)
}

pub fn expand(tracks: &mut [Track], items: &[Contrib], gap: Au, avail: Avail) {
    if !tracks.iter().any(|t| flex(t).is_some()) {
        return;
    }
    let n = tracks.iter().filter(|t| !t.collapsed).count();
    let gaps = Au(gap.0.saturating_mul(n.saturating_sub(1) as i32));
    let fr = match avail {
        Avail::MinContent => return,
        Avail::Definite(v) => {
            let used = tracks.iter().fold(gaps, |a, t| a + t.base);
            if v - used <= Au::ZERO {
                return;
            }
            find_fr(tracks, 0..tracks.len(), v - gaps)
        }
        Avail::MaxContent => {
            let mut fr = 0.0f64;
            for t in tracks.iter() {
                if let Some(f) = flex(t) {
                    let (b, f) = (f64::from(t.base.0), f64::from(f));
                    fr = fr.max(if f > 1.0 { b / f } else { b });
                }
            }
            for c in items {
                if tracks[c.first..c.end].iter().any(|t| flex(t).is_some()) {
                    let g = Au(gap.0.saturating_mul((c.end - c.first - 1) as i32));
                    fr = fr.max(find_fr(tracks, c.first..c.end, c.max_content - g));
                }
            }
            fr
        }
    };
    // each flexed track takes what its share brings the running total to, so the shares add up to the
    // space they fill and the last one takes what truncating left over
    let (mut sum, mut given) = (0.0f64, 0i64);
    for t in tracks.iter_mut() {
        if let Some(f) = flex(t)
            && fr * f64::from(f) > f64::from(t.base.0)
        {
            sum += fr * f64::from(f);
            let upto = (sum + 1e-6).floor() as i64;
            let v = (upto - given).clamp(0, i64::from(i32::MAX)) as i32;
            t.base = Au(v).max(t.base);
            given = upto;
        }
    }
}
