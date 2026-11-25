//! Column widths: each column's minimum and maximum from its cells and column elements, with spanning
//! cells spreading what their columns lack, then the table's width shared out among them as WebKit's
//! automatic table layout does (percentage columns, then fixed, then auto in proportion to their
//! maximums), or by the first row in the fixed table layout.

use madcawl_css::style::Size as CssSize;

use crate::au::Au;
use crate::cx::Cx;
use crate::intrinsic;

use super::structure::Structure;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Auto,
    /// A border-box width
    Fixed(Au),
    Percent(f32),
}

#[derive(Clone, Copy, Debug)]
pub struct Col {
    pub min: Au,
    pub max: Au,
    pub kind: Kind,
    /// Only empty cells start in it, or none
    pub empty: bool,
}

// the border-box width a cell's or column's `width` gives, or its percentage
fn kind_of(w: CssSize, bp: Au, border_box: bool) -> Kind {
    match w {
        CssSize::Lp(l) if l.has_pct() && l.px == 0 => Kind::Percent(l.pct()),
        CssSize::Lp(l) if !l.has_pct() && l.px > 0 => {
            let v = Au(l.px);
            Kind::Fixed(if border_box { v.max(bp) } else { v + bp })
        }
        _ => Kind::Auto,
    }
}

// the stronger of two kinds: a percentage, else the larger fixed width
fn stronger(a: Kind, b: Kind) -> Kind {
    match (a, b) {
        (Kind::Percent(x), Kind::Percent(y)) => Kind::Percent(x.max(y)),
        (Kind::Percent(_), _) => a,
        (_, Kind::Percent(_)) => b,
        (Kind::Fixed(x), Kind::Fixed(y)) => Kind::Fixed(x.max(y)),
        (Kind::Fixed(_), _) => a,
        _ => b,
    }
}

/// A cell's border-box minimum and maximum widths and what its `width` asks; `bp` its borders and padding.
pub fn cell(cx: &mut Cx, b: u32, bp: Au) -> (Au, Au, Kind) {
    let (lo, hi) = intrinsic::content_widths(cx, b, Au::ZERO);
    let (min, max) = (lo + bp, hi + bp);
    if !cx.has_edges(b) {
        return (min, max, Kind::Auto);
    }
    let s = cx.style(b);
    let border_box = s.boxm.box_sizing == madcawl_css::style::BoxSizing::BorderBox;
    let k = kind_of(s.boxm.width, bp, border_box);
    match k {
        Kind::Fixed(w) => (min, w.max(min), k),
        _ => (min, max, k),
    }
}

/// Spreads `extra` over `cols` in proportion to `weight`, equally when they weigh nothing, the last
/// taking what rounding left.
fn spread(
    cols: &mut [Col],
    extra: Au,
    weight: impl Fn(&Col) -> Au,
    field: fn(&mut Col) -> &mut Au,
) {
    if cols.is_empty() || extra <= Au::ZERO {
        return;
    }
    let total: i64 = cols.iter().map(|c| i64::from(weight(c).0)).sum();
    let (n, len) = (cols.len() as i64, cols.len());
    let mut given = 0i64;
    for (k, c) in cols.iter_mut().enumerate() {
        let share = if total > 0 {
            i64::from(extra.0) * i64::from(weight(c).0) / total
        } else {
            i64::from(extra.0) / n
        };
        let share = if k + 1 == len {
            i64::from(extra.0) - given
        } else {
            share
        };
        given += share;
        *field(c) += Au(share as i32);
    }
}

/// The columns of a table: their widths from single-column cells and column elements, then the cells
/// spanning several columns, narrowest first.
pub fn columns(
    cx: &mut Cx,
    st: &Structure,
    spacing: Au,
    bp: &dyn Fn(&mut Cx, usize) -> Au,
) -> Vec<Col> {
    let mut cols = vec![
        Col {
            min: Au::ZERO,
            max: Au::ZERO,
            kind: Kind::Auto,
            empty: true,
        };
        st.ncols
    ];
    for (k, w) in st.col_widths.iter().enumerate() {
        if let Some(w) = w {
            cols[k].kind = kind_of(*w, Au::ZERO, true);
        }
    }
    let mut spanning = Vec::new();
    for (i, c) in st.cells.iter().enumerate() {
        let p = bp(cx, i);
        let (min, max, kind) = cell(cx, c.b, p);
        if c.cols > 1 {
            spanning.push((i, min, max));
            continue;
        }
        let col = &mut cols[c.col];
        col.empty &= cx.tree.get(c.b).first == crate::tree::NONE;
        col.min = col.min.max(min);
        col.max = col.max.max(max);
        col.kind = stronger(col.kind, kind);
    }
    for c in cols.iter_mut() {
        if let Kind::Fixed(w) = c.kind {
            c.max = c.max.max(w);
        }
        c.max = c.max.max(c.min);
    }
    spanning.sort_by_key(|&(i, _, _)| st.cells[i].cols);
    for (i, min, max) in spanning {
        let c = &st.cells[i];
        let empty = cx.tree.get(c.b).first == crate::tree::NONE;
        let range = &mut cols[c.col..c.col + c.cols];
        range.iter_mut().for_each(|x| x.empty &= empty);
        let gaps = Au(spacing.0.saturating_mul(c.cols as i32 - 1));
        // what the columns lack goes to the auto ones, else to all in proportion to their maximums
        let any_auto = range.iter().any(|x| x.kind == Kind::Auto);
        let to = |k: &Kind| !any_auto || *k == Kind::Auto;
        // a fixed column's minimum first grows to its width, as WebKit gives it
        let mut lack = min - range.iter().fold(gaps, |a, x| a + x.min);
        for x in range.iter_mut() {
            if let Kind::Fixed(w) = x.kind
                && lack > Au::ZERO
            {
                let d = (w - x.min).max(Au::ZERO).min(lack);
                x.min += d;
                lack -= d;
            }
        }
        let mut sub: Vec<Col> = range.iter().filter(|x| to(&x.kind)).copied().collect();
        spread(&mut sub, lack, |x| x.max, |x| &mut x.min);
        let have_max = range.iter().fold(gaps, |a, x| a + x.max);
        spread(&mut sub, max - have_max, |x| x.max, |x| &mut x.max);
        let mut it = sub.into_iter();
        for x in range.iter_mut().filter(|x| to(&x.kind)) {
            *x = it.next().expect("one per picked column");
        }
        for x in range.iter_mut() {
            x.max = x.max.max(x.min);
        }
    }
    cols
}

/// The minimum and maximum widths of the columns with the spacing around them.
pub fn total(cols: &[Col], spacing: Au) -> (Au, Au) {
    let s = Au(spacing
        .0
        .saturating_mul(cols.len() as i32 + i32::from(!cols.is_empty())));
    let min = cols.iter().fold(s, |a, c| a + c.min);
    let max = cols.iter().fold(s, |a, c| a + c.max);
    (min, max)
}

// `extra` in proportion to the columns' widths, equally when they have none
pub fn give(w: &mut [Au], to: &[usize], extra: Au) {
    let total: i64 = to.iter().map(|&k| i64::from(w[k].0)).sum();
    let (mut given, n) = (0i64, to.len() as i64);
    for (j, &k) in to.iter().enumerate() {
        let share = if j + 1 == to.len() {
            i64::from(extra.0) - given
        } else if total > 0 {
            i64::from(extra.0) * i64::from(w[k].0) / total
        } else {
            i64::from(extra.0) / n
        };
        given += share;
        w[k] += Au(share as i32);
    }
}

/// The fixed table layout: the columns' widths from the column elements and the first row's cells, the
/// rest of `avail` shared equally among the columns that give none.
pub fn fixed(cx: &mut Cx, st: &Structure, avail: Au, bp: &dyn Fn(&mut Cx, usize) -> Au) -> Vec<Au> {
    let mut kinds: Vec<Kind> = (0..st.ncols)
        .map(|k| {
            st.col_widths
                .get(k)
                .copied()
                .flatten()
                .map_or(Kind::Auto, |w| kind_of(w, Au::ZERO, true))
        })
        .collect();
    for (i, c) in st.cells.iter().enumerate().filter(|(_, c)| c.row == 0) {
        if c.cols == 1 && kinds[c.col] == Kind::Auto && cx.has_edges(c.b) {
            let s = cx.style(c.b);
            let border_box = s.boxm.box_sizing == madcawl_css::style::BoxSizing::BorderBox;
            let p = bp(cx, i);
            kinds[c.col] = kind_of(s.boxm.width, p, border_box);
        }
    }
    let mut w: Vec<Au> = kinds
        .iter()
        .map(|k| match *k {
            Kind::Fixed(v) => v,
            Kind::Percent(p) => Au::from_px_trunc(avail.px() * p / 100.0),
            Kind::Auto => Au::ZERO,
        })
        .collect();
    let used = w.iter().fold(Au::ZERO, |a, &x| a + x);
    let autos: Vec<usize> = (0..w.len()).filter(|&k| kinds[k] == Kind::Auto).collect();
    let left = (avail - used).max(Au::ZERO);
    if !autos.is_empty() {
        let each = Au(left.0 / autos.len() as i32);
        for (j, &k) in autos.iter().enumerate() {
            w[k] = if j + 1 == autos.len() {
                left - Au(each.0 * (autos.len() as i32 - 1))
            } else {
                each
            };
        }
    } else if left > Au::ZERO && !w.is_empty() {
        let all: Vec<usize> = (0..w.len()).collect();
        give(&mut w, &all, left);
    }
    w
}
