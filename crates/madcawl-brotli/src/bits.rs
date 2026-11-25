//! The bit reader over the decoder's own input buffer, least significant
//! bit first.
//!
//! Running out of input does not fail on the spot: the read sets `short`
//! and goes on with zeros, and the caller, which took a [`Mark`] before
//! the unit it is reading, rolls back to it and asks for more input. A
//! unit is at most a few hundred bytes, so the buffer always holds one.

pub(crate) const IN_CAP: usize = 1 << 16;

pub(crate) struct Bits {
    buf: Box<[u8]>,
    pos: usize,
    end: usize,
    /// bits not yet read are the low `nbits`; the bits above them are
    /// either zero or the bytes at `pos` onwards
    val: u64,
    nbits: u32,
    pub short: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Mark {
    pos: usize,
    val: u64,
    nbits: u32,
}

impl Bits {
    pub fn new() -> Self {
        Bits { buf: vec![0; IN_CAP].into_boxed_slice(), pos: 0, end: 0, val: 0, nbits: 0, short: false }
    }

    pub fn mark(&self) -> Mark {
        Mark { pos: self.pos, val: self.val, nbits: self.nbits }
    }

    pub fn reset(&mut self, m: Mark) {
        self.pos = m.pos;
        self.val = m.val;
        self.nbits = m.nbits;
        self.short = false;
    }

    /// Whole bytes taken in but not yet read
    pub fn unread(&self) -> usize {
        self.end - self.pos + (self.nbits / 8) as usize
    }

    /// Takes as much of `input` as fits and returns how much that was
    pub fn feed(&mut self, input: &[u8]) -> usize {
        if self.pos > 0 && IN_CAP - self.end < input.len() {
            self.buf.copy_within(self.pos..self.end, 0);
            self.end -= self.pos;
            self.pos = 0;
        }
        let n = input.len().min(IN_CAP - self.end);
        self.buf[self.end..self.end + n].copy_from_slice(&input[..n]);
        self.end += n;
        n
    }

    #[inline]
    fn refill(&mut self) {
        if self.end - self.pos >= 8 {
            let w = u64::from_le_bytes(self.buf[self.pos..self.pos + 8].try_into().expect("8 bytes"));
            self.val |= w << self.nbits;
            let k = (63 - self.nbits) >> 3;
            self.pos += k as usize;
            self.nbits += k * 8;
        } else {
            while self.nbits <= 56 && self.pos < self.end {
                self.val |= u64::from(self.buf[self.pos]) << self.nbits;
                self.pos += 1;
                self.nbits += 8;
            }
        }
    }

    /// The next `n` (at most 32) bits without reading them
    #[inline]
    pub fn peek(&mut self, n: u32) -> u64 {
        if self.nbits < n {
            self.refill();
        }
        self.val & ((1u64 << n) - 1)
    }

    #[inline]
    pub fn skip_bits(&mut self, n: u32) {
        if n > self.nbits {
            self.short = true;
            self.nbits = 0;
        } else {
            self.nbits -= n;
        }
        self.val >>= n;
    }

    #[inline]
    pub fn take(&mut self, n: u32) -> u32 {
        let v = self.peek(n) as u32;
        self.skip_bits(n);
        v
    }

    /// Reads the bits up to the next byte boundary
    pub fn align(&mut self) -> u32 {
        self.take(self.nbits & 7)
    }

    /// Copies whole bytes to `dst` (the reader is at a byte boundary)
    /// and returns how many there were
    pub fn bytes(&mut self, dst: &mut [u8]) -> usize {
        let mut i = 0;
        while i < dst.len() && self.nbits >= 8 {
            dst[i] = self.val as u8;
            self.val >>= 8;
            self.nbits -= 8;
            i += 1;
        }
        if i == dst.len() {
            return i;
        }
        // what is above `nbits` is about to be read past
        self.val = 0;
        let n = (dst.len() - i).min(self.end - self.pos);
        dst[i..i + n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        i + n
    }

    /// Skips up to `n` whole bytes and returns how many it skipped
    pub fn skip_bytes(&mut self, n: usize) -> usize {
        let mut i = 0;
        while i < n && self.nbits >= 8 {
            self.val >>= 8;
            self.nbits -= 8;
            i += 1;
        }
        if i == n {
            return i;
        }
        self.val = 0;
        let k = (n - i).min(self.end - self.pos);
        self.pos += k;
        i + k
    }
}
