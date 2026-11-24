//! Ids and classes as quirks mode compares them: ASCII
//! case-insensitively, over the UTF-16 text of the attribute; and
//! attribute values as attribute selectors compare them.

use std::ops::Range;

use madcawl_str::DomString;

use crate::selector::AttrOp;

fn fold(u: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&u) {
        u + 32
    } else {
        u
    }
}

/// A hash of the UTF-16 `units` that ignores ASCII case.
pub(crate) fn caseless_hash(units: impl Iterator<Item = u16>) -> u64 {
    units.fold(0xcbf2_9ce4_8422_2325, |h, u| {
        (h ^ u64::from(fold(u))).wrapping_mul(0x100_0000_01b3)
    })
}

/// The ASCII-whitespace-separated words of `s`, as ranges of its units.
pub(crate) fn words(s: &DomString) -> impl Iterator<Item = Range<usize>> + '_ {
    let space = |u: u16| matches!(u, 0x09 | 0x0a | 0x0c | 0x0d | 0x20);
    let n = s.len();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < n && space(s.unit(i)) {
            i += 1;
        }
        let start = i;
        while i < n && !space(s.unit(i)) {
            i += 1;
        }
        (start < i).then_some(start..i)
    })
}

pub(crate) fn caseless_eq(s: &DomString, r: Range<usize>, want: &str) -> bool {
    r.map(|i| fold(s.unit(i))).eq(want.encode_utf16().map(fold))
}

/// An attribute selector's operator applied to the value `have`.
pub(crate) fn value_matches(op: AttrOp, have: &str, want: &str, caseless: bool) -> bool {
    let (h, w) = (have.as_bytes(), want.as_bytes());
    let eq = |a: &[u8]| same(a, w, caseless);
    let starts = h.len() >= w.len() && eq(&h[..w.len()]);
    match op {
        AttrOp::Equals => eq(h),
        AttrOp::Includes => {
            !w.is_empty()
                && !w.iter().any(u8::is_ascii_whitespace)
                && h.split(u8::is_ascii_whitespace).any(eq)
        }
        AttrOp::DashMatch => eq(h) || starts && h.get(w.len()) == Some(&b'-'),
        AttrOp::Prefix => !w.is_empty() && starts,
        AttrOp::Suffix => !w.is_empty() && h.len() >= w.len() && eq(&h[h.len() - w.len()..]),
        AttrOp::Substring => !w.is_empty() && h.windows(w.len()).any(eq),
    }
}

/// `have` is `want`, ASCII case-insensitively when `caseless`.
fn same(have: &[u8], want: &[u8], caseless: bool) -> bool {
    if caseless {
        have.eq_ignore_ascii_case(want)
    } else {
        have == want
    }
}
