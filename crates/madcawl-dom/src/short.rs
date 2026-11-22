//! Short strings seen lately, shared: an attribute value or a text that
//! a page repeats (`true`, `button`, `_blank`) takes a count on one
//! string instead of a block of its own. The table is direct-mapped by
//! a hash of the bytes, so values that collide only miss.

use madcawl_str::DomString;

const ENTRIES: usize = 1024;
pub(crate) const SHORT_MAX: usize = 24;

struct Entry {
    len: u8,
    bytes: [u8; SHORT_MAX],
    s: Option<DomString>,
}

#[derive(Default)]
pub(crate) struct ShortStrings {
    entries: Option<Box<[Entry]>>,
}

impl ShortStrings {
    /// The string of `v`, WTF-8: a shared one when `v` is short.
    pub(crate) fn get(&mut self, v: &[u8]) -> DomString {
        if v.is_empty() || v.len() > SHORT_MAX {
            return DomString::from_wtf8(v);
        }
        let entries = self.entries.get_or_insert_with(|| {
            (0..ENTRIES)
                .map(|_| Entry {
                    len: 0,
                    bytes: [0; SHORT_MAX],
                    s: None,
                })
                .collect()
        });
        let e = &mut entries[hash(v)];
        if usize::from(e.len) == v.len()
            && e.bytes[..v.len()] == *v
            && let Some(s) = &e.s
        {
            return s.clone();
        }
        let s = DomString::from_wtf8(v);
        e.len = v.len() as u8;
        e.bytes[..v.len()].copy_from_slice(v);
        e.s = Some(s.clone());
        s
    }
}

/// From the first and the last eight bytes, which overlap or are
/// padded with zeros on a shorter value, and the length.
fn hash(v: &[u8]) -> usize {
    let word = |b: &[u8]| {
        let mut w = [0u8; 8];
        w[..b.len()].copy_from_slice(b);
        u64::from_ne_bytes(w)
    };
    let n = v.len();
    let (a, b) = if n >= 8 {
        (word(&v[..8]), word(&v[n - 8..]))
    } else {
        (word(v), 0)
    };
    let x = (a ^ b.rotate_left(29) ^ n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (x >> (64 - ENTRIES.trailing_zeros())) as usize
}
