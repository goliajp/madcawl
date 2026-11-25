//! Frames between content processes and the network process: a length
//! (u32, little-endian, of what follows), a kind (u16), then fields, each
//! a tag byte, a LEB128 length and the bytes; the same layout as the agent
//! protocol. A tag may repeat. A reader skips kinds and tags it does not
//! know.

use std::io::{self, Read, Write};

/// The longest frame a reader takes; a longer length is an error, never
/// an allocation.
pub const MAX_FRAME: usize = 16 << 20;
pub const MAX_FIELDS: usize = 1024;
/// Response bodies go out in frames of at most this many bytes.
pub const BODY_CHUNK: usize = 256 << 10;

pub mod kind {
    /// content → network: start a fetch
    pub const FETCH: u16 = 1;
    /// network → content: status, final URL and the headers it may see
    pub const HEAD: u16 = 2;
    pub const BODY: u16 = 3;
    pub const END: u16 = 4;
    /// a failed fetch; it may follow a HEAD and BODY frames when the
    /// response fails part way, and nothing follows it
    pub const ERROR: u16 = 5;
    /// content → network: `document.cookie` read and write
    pub const COOKIE_GET: u16 = 6;
    pub const COOKIE_SET: u16 = 7;
    pub const COOKIES: u16 = 8;
}

pub mod tag {
    pub const ID: u8 = 1;
    pub const METHOD: u8 = 2;
    pub const URL: u8 = 3;
    /// name, a zero byte, value
    pub const HEADER: u8 = 4;
    pub const BODY: u8 = 5;
    pub const NAVIGATION: u8 = 6;
    pub const INITIATOR: u8 = 7;
    pub const CREDENTIALS: u8 = 8;
    pub const REDIRECT: u8 = 9;
    pub const STATUS: u8 = 10;
    pub const REASON: u8 = 11;
    pub const MESSAGE: u8 = 12;
    pub const TEXT: u8 = 13;
    pub const OK: u8 = 14;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: u16,
    pub fields: Vec<(u8, Vec<u8>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    Short,
    TooLong,
    Malformed,
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn get_varint(b: &[u8]) -> Option<(u64, usize)> {
    let mut v = 0u64;
    for (i, &x) in b.iter().enumerate().take(10) {
        v |= u64::from(x & 0x7f) << (7 * i);
        if x & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

impl Frame {
    pub fn new(kind: u16) -> Self {
        Frame { kind, fields: Vec::new() }
    }

    pub fn bytes(mut self, tag: u8, v: &[u8]) -> Self {
        self.fields.push((tag, v.to_vec()));
        self
    }

    pub fn str(self, tag: u8, v: &str) -> Self {
        self.bytes(tag, v.as_bytes())
    }

    pub fn num(mut self, tag: u8, v: u64) -> Self {
        let mut b = Vec::new();
        put_varint(&mut b, v);
        self.fields.push((tag, b));
        self
    }

    pub fn header(self, name: &str, value: &[u8]) -> Self {
        let mut b = Vec::with_capacity(name.len() + 1 + value.len());
        b.extend_from_slice(name.as_bytes());
        b.push(0);
        b.extend_from_slice(value);
        self.bytes(tag::HEADER, &b)
    }

    pub fn field(&self, tag: u8) -> Option<&[u8]> {
        self.fields.iter().find(|(t, _)| *t == tag).map(|(_, v)| v.as_slice())
    }

    pub fn all(&self, tag: u8) -> impl Iterator<Item = &[u8]> {
        self.fields.iter().filter(move |(t, _)| *t == tag).map(|(_, v)| v.as_slice())
    }

    pub fn get_str(&self, tag: u8) -> Option<&str> {
        std::str::from_utf8(self.field(tag)?).ok()
    }

    pub fn get_num(&self, tag: u8) -> Option<u64> {
        let b = self.field(tag)?;
        get_varint(b).filter(|&(_, n)| n == b.len()).map(|(v, _)| v)
    }

    /// The headers, each split at its first zero byte; others are skipped.
    pub fn headers(&self) -> Vec<(String, Vec<u8>)> {
        self.all(tag::HEADER)
            .filter_map(|b| {
                let i = b.iter().position(|&x| x == 0)?;
                Some((std::str::from_utf8(&b[..i]).ok()?.to_owned(), b[i + 1..].to_vec()))
            })
            .collect()
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&self.kind.to_le_bytes());
        for (tag, v) in &self.fields {
            out.push(*tag);
            put_varint(out, v.len() as u64);
            out.extend_from_slice(v);
        }
        let len = (out.len() - start - 4) as u32;
        out[start..start + 4].copy_from_slice(&len.to_le_bytes());
    }

    /// The frame at the start of `b` and how many bytes it took.
    pub fn decode(b: &[u8]) -> Result<(Frame, usize), FrameError> {
        let Some(head) = b.first_chunk::<4>() else { return Err(FrameError::Short) };
        let len = u32::from_le_bytes(*head) as usize;
        if len > MAX_FRAME {
            return Err(FrameError::TooLong);
        }
        if len < 2 {
            return Err(FrameError::Malformed);
        }
        let body = b.get(4..4 + len).ok_or(FrameError::Short)?;
        Ok((Frame { kind: u16::from_le_bytes([body[0], body[1]]), fields: fields(&body[2..])? }, 4 + len))
    }

    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        let mut out = Vec::new();
        self.encode(&mut out);
        w.write_all(&out)
    }

    /// Reads one frame; `None` at a clean end of the stream. The length
    /// is the peer's word, so the buffer grows with the bytes that arrive
    /// rather than being reserved up front, and the largest field keeps
    /// that buffer instead of being copied out of it.
    pub fn read_from(r: &mut impl Read) -> io::Result<Option<Frame>> {
        let mut head = [0u8; 4];
        match r.read_exact(&mut head) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let len = u32::from_le_bytes(head) as usize;
        if !(2..=MAX_FRAME).contains(&len) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "frame length out of range"));
        }
        let mut body = Vec::new();
        // bytes read so far; the buffer past them is zeroed once, when it
        // grows, not again on every short read
        let mut filled = 0;
        while filled < len {
            if filled == body.len() {
                // doubling, but taking the rest at once when it is less
                // than two steps, so the last growth is not a small one
                let step = filled.max(64 << 10);
                let want = if len - filled <= 2 * step { len - filled } else { step };
                body.reserve_exact(want);
                body.resize(filled + want, 0);
            }
            filled += match r.read(&mut body[filled..]) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => 0,
                Err(e) => return Err(e),
            };
        }
        let kind = u16::from_le_bytes([body[0], body[1]]);
        let spans = spans(&body, 2).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?;
        let largest = (0..spans.len()).max_by_key(|&i| spans[i].2 - spans[i].1);
        let mut fields: Vec<(u8, Vec<u8>)> = spans
            .iter()
            .enumerate()
            .map(|(i, &(t, from, to))| (t, if Some(i) == largest { Vec::new() } else { body[from..to].to_vec() }))
            .collect();
        if let Some(i) = largest {
            let (_, from, to) = spans[i];
            body.truncate(to);
            body.drain(..from);
            body.shrink_to_fit();
            fields[i].1 = body;
        }
        Ok(Some(Frame { kind, fields }))
    }

    /// Takes out the first field with `tag`, without copying it.
    pub fn take(&mut self, tag: u8) -> Option<Vec<u8>> {
        let i = self.fields.iter().position(|(t, _)| *t == tag)?;
        Some(self.fields.remove(i).1)
    }
}

/// Each field of `b` from `at` on: its tag and where its bytes are.
fn spans(b: &[u8], mut at: usize) -> Result<Vec<(u8, usize, usize)>, FrameError> {
    let mut out = Vec::new();
    while at < b.len() {
        if out.len() == MAX_FIELDS {
            return Err(FrameError::Malformed);
        }
        let tag = b[at];
        let (n, used) = get_varint(&b[at + 1..]).ok_or(FrameError::Malformed)?;
        let from = at + 1 + used;
        let to = from
            .checked_add(usize::try_from(n).map_err(|_| FrameError::Malformed)?)
            .filter(|&t| t <= b.len())
            .ok_or(FrameError::Malformed)?;
        out.push((tag, from, to));
        at = to;
    }
    Ok(out)
}

fn fields(body: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, FrameError> {
    Ok(spans(body, 0)?.into_iter().map(|(t, from, to)| (t, body[from..to].to_vec())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_with_repeated_tags() {
        let f = Frame::new(kind::HEAD).num(tag::ID, 7).header("a", b"1").header("b", b"x\0y").str(tag::URL, "u");
        let mut b = Vec::new();
        f.encode(&mut b);
        let (g, used) = Frame::decode(&b).unwrap();
        assert_eq!((g.clone(), used), (f, b.len()));
        assert_eq!(g.headers(), vec![("a".into(), b"1".to_vec()), ("b".into(), b"x\0y".to_vec())]);
        assert_eq!(Frame::read_from(&mut &b[..]).unwrap(), Some(g));
        assert_eq!(Frame::read_from(&mut &b""[..]).unwrap(), None);
    }

    /// A peer sending one byte at a time costs a read per byte, not a
    /// zeroed buffer per byte.
    #[test]
    fn a_frame_sent_a_byte_at_a_time_is_read_in_linear_time() {
        struct Dribble<'a>(&'a [u8]);
        impl Read for Dribble<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                let Some((&b, rest)) = self.0.split_first() else { return Ok(0) };
                buf[0] = b;
                self.0 = rest;
                Ok(1)
            }
        }
        let f = Frame::new(kind::BODY).bytes(tag::BODY, &vec![7u8; 4 << 20]);
        let mut b = Vec::new();
        f.encode(&mut b);
        let t = std::time::Instant::now();
        assert_eq!(Frame::read_from(&mut Dribble(&b)).unwrap(), Some(f));
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn hostile_frames_are_errors() {
        assert_eq!(Frame::decode(&[1, 0]), Err(FrameError::Short));
        assert_eq!(Frame::decode(&[0xff, 0xff, 0xff, 0xff]), Err(FrameError::TooLong));
        assert_eq!(Frame::decode(&[5, 0, 0, 0, 1, 0, 3, 9, 0]), Err(FrameError::Malformed));
        assert!(Frame::read_from(&mut &[0xffu8, 0xff, 0xff, 0x7f][..]).is_err());
        let short = Frame::read_from(&mut &[0x00u8, 0x00, 0x00, 0x01, 1, 0, 9][..]).unwrap_err();
        assert_eq!(short.kind(), io::ErrorKind::UnexpectedEof, "a frame claiming 16 MiB that stops short");
        let mut seed = 0x9e37_79b9u32;
        for n in 0..2000usize {
            let junk: Vec<u8> = (0..n % 64)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                })
                .collect();
            let _ = Frame::decode(&junk);
            let _ = Frame::read_from(&mut &junk[..]);
        }
    }
}
