//! Prefix codes (section 3): reading their description and decoding
//! symbols with a two-level table.
//!
//! A table is an 8-bit root followed by its second-level tables. A root
//! entry with `bits` above 8 points at the second-level table of
//! `bits - 8` index bits at offset `value`; any other entry is a symbol
//! and the length of its code.

use crate::Error;
use crate::bits::Bits;

#[derive(Clone, Copy, Default)]
pub(crate) struct Entry {
    bits: u8,
    value: u16,
}

const ROOT: usize = 256;

/// The largest alphabet, insert-and-copy lengths
pub(crate) const MAX_ALPHABET: usize = 704;

/// Code length code lengths are stored in this order
const ORDER: [usize; 18] = [1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15];

#[inline]
pub(crate) fn symbol(br: &mut Bits, t: &[Entry]) -> usize {
    let v = br.peek(15);
    let mut e = t[(v & 0xff) as usize];
    if e.bits > 8 {
        e = t[e.value as usize + ((v >> 8) & ((1 << (e.bits - 8)) - 1)) as usize];
    }
    br.skip_bits(u32::from(e.bits));
    e.value as usize
}

/// Appends the table of a code with one symbol and no bits
fn single(out: &mut Vec<Entry>, sym: usize) {
    out.extend(std::iter::repeat_n(Entry { bits: 0, value: sym as u16 }, ROOT));
}

fn reversed(code: u32, len: u8) -> usize {
    ((code as u16).reverse_bits() >> (16 - len)) as usize
}

/// Appends the table of the complete canonical code with these lengths
/// (0 for an unused symbol, at most 15)
fn build(out: &mut Vec<Entry>, lens: &[u8]) {
    let base = out.len();
    let mut count = [0u32; 16];
    for &l in lens {
        count[l as usize] += 1;
    }
    count[0] = 0;
    let mut first = [0u32; 16];
    for l in 1..16 {
        first[l] = (first[l - 1] + count[l - 1]) << 1;
    }
    // the longest code under each root entry that needs a second level
    let mut longest = [0u8; ROOT];
    let mut next = first;
    for &l in lens.iter().filter(|&&l| l > 0) {
        let r = reversed(next[l as usize], l) & 0xff;
        next[l as usize] += 1;
        longest[r] = longest[r].max(l);
    }
    out.resize(base + ROOT, Entry::default());
    for (r, &l) in longest.iter().enumerate() {
        if l > 8 {
            let at = out.len() - base;
            out[base + r] = Entry { bits: l, value: at as u16 };
            out.resize(out.len() + (1 << (l - 8)), Entry::default());
        }
    }
    let mut next = first;
    for (sym, &l) in lens.iter().enumerate().filter(|(_, l)| **l > 0) {
        let r = reversed(next[l as usize], l);
        next[l as usize] += 1;
        let e = Entry { bits: l, value: sym as u16 };
        let (start, size, mut i, step) = if l <= 8 {
            (base, ROOT, r, 1 << l)
        } else {
            let root = out[base + (r & 0xff)];
            (base + root.value as usize, 1 << (root.bits - 8), r >> 8, 1 << (l - 8))
        };
        while i < size {
            out[start + i] = e;
            i += step;
        }
    }
}

/// Reads a prefix code over `alphabet` symbols and appends its table;
/// `lens` and `cl` are scratch space
pub(crate) fn read(
    br: &mut Bits,
    alphabet: usize,
    lens: &mut [u8; MAX_ALPHABET],
    cl: &mut Vec<Entry>,
    out: &mut Vec<Entry>,
) -> Result<(), Error> {
    let lens = &mut lens[..alphabet];
    lens.fill(0);
    let hskip = br.take(2);
    if hskip == 1 {
        return read_simple(br, lens, out);
    }
    let mut cl_lens = [0u8; 18];
    let mut space = 32i32;
    let mut used = 0;
    for &sym in &ORDER[hskip as usize..] {
        let v = code_length_length(br);
        cl_lens[sym] = v;
        if v != 0 {
            space -= 32 >> v;
            used += 1;
            if space <= 0 {
                break;
            }
        }
    }
    if used != 1 && space != 0 {
        return Err(Error::Format("bad code length code"));
    }
    cl.clear();
    match cl_lens.iter().position(|&l| l != 0) {
        Some(s) if used == 1 => single(cl, s),
        _ => build(cl, &cl_lens),
    }
    read_lengths(br, cl, lens)?;
    build(out, lens);
    Ok(())
}

/// The fixed code of code length code lengths: 0 "00", 3 "10", 4 "01",
/// 2 "011", 1 "0111", 5 "1111", read right to left
fn code_length_length(br: &mut Bits) -> u8 {
    let v = br.peek(4);
    let (n, len) = match v & 3 {
        0 => (2, 0),
        2 => (2, 3),
        1 => (2, 4),
        _ if v & 4 == 0 => (3, 2),
        _ if v & 8 == 0 => (4, 1),
        _ => (4, 5),
    };
    br.skip_bits(n);
    len
}

fn read_lengths(br: &mut Bits, cl: &[Entry], lens: &mut [u8]) -> Result<(), Error> {
    let mut space = 32768i32;
    let mut at = 0;
    let mut prev = 8u8;
    let (mut repeat, mut repeat_len) = (0usize, 0u8);
    while at < lens.len() && space > 0 {
        let p = symbol(br, cl) as u8;
        if p < 16 {
            repeat = 0;
            lens[at] = p;
            at += 1;
            if p != 0 {
                prev = p;
                space -= 32768 >> p;
            }
            continue;
        }
        let (extra, len) = if p == 16 { (2, prev) } else { (3, 0) };
        if repeat_len != len {
            repeat = 0;
            repeat_len = len;
        }
        let old = repeat;
        if repeat > 0 {
            repeat = (repeat - 2) << extra;
        }
        repeat += br.take(extra) as usize + 3;
        let delta = repeat - old;
        if at + delta > lens.len() {
            return Err(Error::Format("code lengths past the alphabet"));
        }
        lens[at..at + delta].fill(len);
        at += delta;
        if len != 0 {
            space -= (delta as i32) * (32768 >> len);
        }
    }
    if space != 0 {
        return Err(Error::Format("incomplete prefix code"));
    }
    Ok(())
}

fn read_simple(br: &mut Bits, lens: &mut [u8], out: &mut Vec<Entry>) -> Result<(), Error> {
    let n = br.take(2) as usize + 1;
    let width = usize::BITS - (lens.len() - 1).leading_zeros();
    let mut syms = [0usize; 4];
    for i in 0..n {
        let s = br.take(width) as usize;
        if s >= lens.len() || syms[..i].contains(&s) {
            return Err(Error::Format("bad simple prefix code"));
        }
        syms[i] = s;
    }
    let shape: &[u8] = match n {
        1 => {
            single(out, syms[0]);
            return Ok(());
        }
        2 => &[1, 1],
        3 => &[1, 2, 2],
        _ if br.take(1) == 0 => &[2, 2, 2, 2],
        _ => &[1, 2, 3, 3],
    };
    for (&s, &l) in syms.iter().zip(shape) {
        lens[s] = l;
    }
    build(out, lens);
    Ok(())
}
