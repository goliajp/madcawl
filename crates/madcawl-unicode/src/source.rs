//! Random access to the code points of a text, in whatever encoding it is stored.

/// Text as a sequence of code points addressed by offsets in its own code units. Offsets passed in are always
/// ones this source returned, or 0 and `len()`.
pub trait Source {
    /// Length in code units.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The code point starting at `i < len()` and the offset just after it.
    fn next(&self, i: usize) -> (u32, usize);

    /// The code point ending at `0 < i` and the offset where it starts.
    fn prev(&self, i: usize) -> (u32, usize);
}

impl Source for str {
    fn len(&self) -> usize {
        str::len(self)
    }

    #[inline]
    fn next(&self, i: usize) -> (u32, usize) {
        let b = self.as_bytes();
        let c = b[i] as u32;
        if c < 0x80 {
            return (c, i + 1);
        }
        let (n, mut v) = match c {
            0xC0..=0xDF => (2, c & 0x1F),
            0xE0..=0xEF => (3, c & 0x0F),
            _ => (4, c & 0x07),
        };
        for k in 1..n {
            v = v << 6 | (b[i + k] as u32 & 0x3F);
        }
        (v, i + n)
    }

    #[inline]
    fn prev(&self, i: usize) -> (u32, usize) {
        let b = self.as_bytes();
        let mut s = i - 1;
        while b[s] & 0xC0 == 0x80 {
            s -= 1;
        }
        (self.next(s).0, s)
    }
}

/// UTF-16 code units; a lone surrogate reads as U+FFFD.
#[derive(Clone, Copy)]
pub struct Utf16<'a>(pub &'a [u16]);

impl Source for Utf16<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    fn next(&self, i: usize) -> (u32, usize) {
        let u = self.0[i] as u32;
        match u {
            0xD800..=0xDBFF => match self.0.get(i + 1) {
                Some(&l @ 0xDC00..=0xDFFF) => {
                    (0x10000 + ((u - 0xD800) << 10) + (l as u32 - 0xDC00), i + 2)
                }
                _ => (0xFFFD, i + 1),
            },
            0xDC00..=0xDFFF => (0xFFFD, i + 1),
            _ => (u, i + 1),
        }
    }

    #[inline]
    fn prev(&self, i: usize) -> (u32, usize) {
        let u = self.0[i - 1];
        if (0xDC00..=0xDFFF).contains(&u) && i >= 2 && (0xD800..=0xDBFF).contains(&self.0[i - 2]) {
            return (self.next(i - 2).0, i - 2);
        }
        (self.next(i - 1).0, i - 1)
    }
}

/// Code points, one per unit; values that are not scalar values are passed through as they are.
#[derive(Clone, Copy)]
pub struct Utf32<'a>(pub &'a [u32]);

impl Source for Utf32<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    fn next(&self, i: usize) -> (u32, usize) {
        (self.0[i], i + 1)
    }

    #[inline]
    fn prev(&self, i: usize) -> (u32, usize) {
        (self.0[i - 1], i - 1)
    }
}
