//! Sharing a table's width out among its columns as WebKit's automatic table layout does: every column
//! its minimum, then percentage columns their share of the width, fixed columns their width, auto columns
//! the rest in proportion to their maximums; what is left over goes to fixed, then percentage, then all
//! columns, and what was given beyond the width is taken back from auto, fixed, then percentage columns
//! above their minimums. The arithmetic is in `f32` px and each width rounds up to the unit, as there.

use crate::au::Au;

use super::widths::{Col, Kind};

fn px(v: Au) -> f32 {
    v.px()
}

// takes `avail` (negative) back from the columns of `kind`, in proportion to what each has beyond its
// minimum, from the last
fn shrink(cols: &[Col], w: &mut [f32], avail: &mut f32, is: impl Fn(Kind) -> bool) {
    let mut beyond: f32 = (0..cols.len())
        .filter(|&i| is(cols[i].kind))
        .map(|i| w[i] - px(cols[i].min))
        .sum();
    for i in (0..cols.len()).rev() {
        if beyond <= 0.0 {
            break;
        }
        if !is(cols[i].kind) {
            continue;
        }
        let diff = w[i] - px(cols[i].min);
        let reduce = *avail * diff / beyond;
        w[i] += reduce;
        *avail -= reduce;
        beyond -= diff;
        if *avail >= 0.0 {
            break;
        }
    }
}

/// The used widths of the columns in `avail`, the table's width less its borders, padding and spacing.
pub fn distribute(cols: &[Col], avail: Au) -> Vec<Au> {
    let table = px(avail);
    let mut left = table;
    let mut w: Vec<f32> = cols.iter().map(|c| px(c.min)).collect();
    let (mut total_pct, mut num_auto, mut num_fixed) = (0.0f32, 0, 0);
    let (mut total_auto, mut total_fixed, mut alloc_auto, mut empty_auto) =
        (0.0f32, 0.0f32, 0.0f32, 0);
    for (i, c) in cols.iter().enumerate() {
        left -= w[i];
        match c.kind {
            Kind::Percent(p) => total_pct += p,
            Kind::Fixed(_) => (num_fixed, total_fixed) = (num_fixed + 1, total_fixed + px(c.max)),
            Kind::Auto if c.empty => empty_auto += 1,
            Kind::Auto => {
                num_auto += 1;
                total_auto += px(c.max);
                alloc_auto += w[i];
            }
        }
    }
    let has_pct = cols.iter().any(|c| matches!(c.kind, Kind::Percent(_)));
    if left > 0.0 && has_pct {
        for (i, c) in cols.iter().enumerate() {
            if let Kind::Percent(p) = c.kind {
                let v = px(c.min).max(p * table / 100.0);
                left += w[i] - v;
                w[i] = v;
            }
        }
        if total_pct > 100.0 {
            let mut excess = table * (total_pct - 100.0) / 100.0;
            for i in (0..cols.len()).rev() {
                if matches!(cols[i].kind, Kind::Percent(_)) {
                    let reduction = w[i].min(excess);
                    excess -= reduction;
                    let v = px(cols[i].min).max(w[i] - reduction);
                    left += w[i] - v;
                    w[i] = v;
                }
            }
        }
    }
    if left > 0.0 {
        for (i, c) in cols.iter().enumerate() {
            if let Kind::Fixed(f) = c.kind
                && px(f) > w[i]
            {
                left += w[i] - px(f);
                w[i] = px(f);
            }
        }
    }
    if left > 0.0 && num_auto > 0 {
        left += alloc_auto;
        for (i, c) in cols.iter().enumerate() {
            if c.kind == Kind::Auto && total_auto > 0.0 && !c.empty {
                let v = w[i].max(left * px(c.max) / total_auto);
                left -= v;
                total_auto -= px(c.max);
                w[i] = v;
            }
        }
    }
    if left > 0.0 && num_fixed > 0 {
        for (i, c) in cols.iter().enumerate() {
            if matches!(c.kind, Kind::Fixed(_)) {
                let v = left * px(c.max) / total_fixed;
                left -= v;
                total_fixed -= px(c.max);
                w[i] += v;
            }
        }
    }
    if left > 0.0 && has_pct && total_pct < 100.0 {
        for (i, c) in cols.iter().enumerate() {
            if let Kind::Percent(p) = c.kind {
                let v = left * p / total_pct;
                left -= v;
                total_pct -= p;
                w[i] += v;
                if left == 0.0 || total_pct == 0.0 {
                    break;
                }
            }
        }
    }
    if left > 0.0 && cols.len() > empty_auto {
        let mut n = (cols.len() - empty_auto) as f32;
        for i in (0..cols.len()).rev() {
            if cols[i].kind == Kind::Auto && cols[i].empty {
                continue;
            }
            let v = left / n;
            left -= v;
            n -= 1.0;
            w[i] += v;
        }
    }
    if left < 0.0 {
        shrink(cols, &mut w, &mut left, |k| k == Kind::Auto);
    }
    if left < 0.0 {
        shrink(cols, &mut w, &mut left, |k| matches!(k, Kind::Fixed(_)));
    }
    if left < 0.0 {
        shrink(cols, &mut w, &mut left, |k| matches!(k, Kind::Percent(_)));
    }
    w.iter()
        .map(|&v| Au((v * 64.0 - 1e-3).ceil().clamp(0.0, i32::MAX as f32) as i32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(min: f32, max: f32, kind: Kind) -> Col {
        Col {
            min: Au::from_px(min),
            max: Au::from_px(max),
            kind,
            empty: false,
        }
    }

    fn px(w: Vec<Au>) -> Vec<f32> {
        w.iter().map(|v| v.px()).collect()
    }

    #[test]
    fn auto_columns_share_the_width_in_proportion_to_their_maximums() {
        let cols = [col(10.0, 30.0, Kind::Auto), col(10.0, 10.0, Kind::Auto)];
        assert_eq!(px(distribute(&cols, Au::from_px(80.0))), [60.0, 20.0]);
        // less than their maximums: a column held at its minimum is made up for by the others
        let cols = [col(30.0, 300.0, Kind::Auto), col(10.0, 10.0, Kind::Auto)];
        assert_eq!(px(distribute(&cols, Au::from_px(100.0))), [90.0, 10.0]);
    }

    #[test]
    fn percentage_and_fixed_columns_come_first_and_give_back_what_others_need() {
        let cols = [
            col(110.0, 120.0, Kind::Auto),
            col(100.0, 500.0, Kind::Percent(50.0)),
            col(261.0, 261.0, Kind::Auto),
        ];
        // the percentage column's half is more than the others leave it
        assert_eq!(
            px(distribute(&cols, Au::from_px(703.0))),
            [110.0, 332.0, 261.0]
        );
        let cols = [
            col(10.0, 10.0, Kind::Fixed(Au::from_px(100.0))),
            col(5.0, 5.0, Kind::Auto),
        ];
        assert_eq!(px(distribute(&cols, Au::from_px(300.0))), [100.0, 200.0]);
    }

    #[test]
    fn what_is_left_without_auto_columns_goes_to_fixed_ones_and_empty_columns_get_none() {
        let cols = [
            col(10.0, 50.0, Kind::Fixed(Au::from_px(50.0))),
            col(10.0, 150.0, Kind::Fixed(Au::from_px(150.0))),
        ];
        assert_eq!(px(distribute(&cols, Au::from_px(400.0))), [100.0, 300.0]);
        let mut empty = col(2.0, 2.0, Kind::Auto);
        empty.empty = true;
        let cols = [empty, col(10.0, 10.0, Kind::Auto)];
        assert_eq!(px(distribute(&cols, Au::from_px(100.0))), [2.0, 98.0]);
    }
}
