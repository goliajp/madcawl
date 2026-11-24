//! Ids and classes as quirks mode compares them: ASCII
//! case-insensitively, over the UTF-16 text of the attribute.

use std::ops::Range;

use madcawl_str::DomString;

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
