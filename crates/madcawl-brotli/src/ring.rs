//! The sliding window: every byte produced goes into it, back-references
//! read from it, and the caller drains it. A byte is overwritten only
//! after it was handed out.

pub(crate) struct Ring {
    buf: Vec<u8>,
    mask: usize,
    /// bytes produced since the start of the stream
    pub written: usize,
    flushed: usize,
}

impl Ring {
    pub fn new() -> Self {
        Ring { buf: Vec::new(), mask: 0, written: 0, flushed: 0 }
    }

    pub fn is_allocated(&self) -> bool {
        !self.buf.is_empty()
    }

    /// `size` is a power of two
    pub fn allocate(&mut self, size: usize) {
        self.buf = vec![0; size];
        self.mask = size - 1;
    }

    pub fn pending(&self) -> usize {
        self.written - self.flushed
    }

    pub fn free(&self) -> usize {
        self.buf.len() - self.pending()
    }

    /// The byte `back` bytes before the end, 0 before the stream start
    #[inline]
    pub fn last(&self, back: usize) -> u8 {
        self.buf[self.written.wrapping_sub(back) & self.mask]
    }

    #[inline]
    pub fn push(&mut self, b: u8) {
        self.buf[self.written & self.mask] = b;
        self.written += 1;
    }

    /// The free space from the write position to the end of the buffer,
    /// to fill and then `commit`
    pub fn space(&mut self) -> &mut [u8] {
        let at = self.written & self.mask;
        let n = self.free().min(self.buf.len() - at);
        &mut self.buf[at..at + n]
    }

    pub fn commit(&mut self, n: usize) {
        self.written += n;
    }

    /// `s.len()` is at most `free()`
    pub fn write(&mut self, s: &[u8]) {
        for &b in s {
            self.push(b);
        }
    }

    /// Appends `n` bytes (at most `free()`) copied from `dist` bytes back
    /// (at most `written`, less than the buffer size)
    pub fn copy(&mut self, dist: usize, n: usize) {
        let size = self.buf.len();
        let src = self.written.wrapping_sub(dist) & self.mask;
        let dst = self.written & self.mask;
        if src + n <= size && dst + n <= size && src < dst {
            // the source is `dist` behind and repeats with that period, so
            // a run copied from `src` can double every time
            let mut done = 0;
            while done < n {
                let k = (n - done).min(dist + done);
                self.buf.copy_within(src..src + k, dst + done);
                done += k;
            }
        } else if src + n <= size && dst + n <= size && dist >= n {
            self.buf.copy_within(src..src + n, dst);
        } else {
            let (mut s, mut d) = (src, dst);
            for _ in 0..n {
                self.buf[d] = self.buf[s];
                s = (s + 1) & self.mask;
                d = (d + 1) & self.mask;
            }
        }
        self.written += n;
    }

    /// Hands out as much as fits in `out` and returns how much
    pub fn flush(&mut self, out: &mut [u8]) -> usize {
        let mut n = 0;
        while n < out.len() && self.flushed < self.written {
            let at = self.flushed & self.mask;
            let k = self.pending().min(self.buf.len() - at).min(out.len() - n);
            out[n..n + k].copy_from_slice(&self.buf[at..at + k]);
            n += k;
            self.flushed += k;
        }
        n
    }
}
