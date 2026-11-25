//! The agent protocol's frames as this side writes and reads them: a
//! length (u32, little-endian, of what follows), a kind (u16), then
//! fields, each a tag byte, a LEB128 length and the bytes. The engine's
//! agent crate defines the protocol; this copy of the codec keeps the
//! server free of the engine's crates. Unknown fields are carried, not
//! refused.

/// The longest reply read; a longer length is an error, never an
/// allocation.
pub const MAX_FRAME: usize = 16 << 20;
const MAX_FIELDS: usize = 256;

pub mod kind {
    pub const READ: u16 = 10;
    pub const DIFF: u16 = 11;
    pub const RESULT: u16 = 20;
    pub const ERROR: u16 = 21;
    pub const OPEN: u16 = 30;
    pub const OPENED: u16 = 31;
    pub const CLOSE: u16 = 32;
    pub const PAGES: u16 = 33;
}

pub mod tag {
    pub const VIEW: u8 = 2;
    pub const SCOPE: u8 = 3;
    pub const REF: u8 = 4;
    pub const BUDGET: u8 = 5;
    pub const HIDDEN: u8 = 6;
    pub const TEXT: u8 = 7;
    pub const ROLE: u8 = 8;
    pub const LIMIT: u8 = 9;
    pub const SINCE: u8 = 10;
    pub const SEQ: u8 = 11;
    pub const TOKENS: u8 = 12;
    pub const FOLDED: u8 = 13;
    pub const CODE: u8 = 14;
    pub const PAGE: u8 = 15;
    pub const URL: u8 = 16;
    pub const WIDTH: u8 = 17;
    pub const HEIGHT: u8 = 18;
    pub const CHARSET: u8 = 19;
    pub const WHAT: u8 = 20;
    pub const FORMAT: u8 = 21;
    pub const OFFSET: u8 = 22;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: u16,
    pub fields: Vec<(u8, Vec<u8>)>,
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
        v |= ((x & 0x7f) as u64) << (7 * i);
        if x & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

impl Frame {
    pub fn new(kind: u16) -> Self {
        Frame {
            kind,
            fields: Vec::new(),
        }
    }

    pub fn str(mut self, tag: u8, v: &str) -> Self {
        self.fields.push((tag, v.as_bytes().to_vec()));
        self
    }

    pub fn num(mut self, tag: u8, v: u64) -> Self {
        let mut b = Vec::new();
        put_varint(&mut b, v);
        self.fields.push((tag, b));
        self
    }

    pub fn get_str(&self, tag: u8) -> Option<&str> {
        let (_, v) = self.fields.iter().find(|(t, _)| *t == tag)?;
        std::str::from_utf8(v).ok()
    }

    pub fn get_num(&self, tag: u8) -> Option<u64> {
        let (_, b) = self.fields.iter().find(|(t, _)| *t == tag)?;
        get_varint(b).filter(|&(_, n)| n == b.len()).map(|(v, _)| v)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0; 4];
        out.extend_from_slice(&self.kind.to_le_bytes());
        for (tag, v) in &self.fields {
            out.push(*tag);
            put_varint(&mut out, v.len() as u64);
            out.extend_from_slice(v);
        }
        let len = (out.len() - 4) as u32;
        out[..4].copy_from_slice(&len.to_le_bytes());
        out
    }

    /// The frame whose body (what follows the length) is `body`.
    pub fn decode_body(body: &[u8]) -> Option<Frame> {
        let kind = u16::from_le_bytes([*body.first()?, *body.get(1)?]);
        let mut fields = Vec::new();
        let mut at = 2;
        while at < body.len() {
            if fields.len() == MAX_FIELDS {
                return None;
            }
            let (n, used) = get_varint(&body[at + 1..])?;
            let from = at + 1 + used;
            let to = from
                .checked_add(usize::try_from(n).ok()?)
                .filter(|&t| t <= body.len())?;
            fields.push((body[at], body[from..to].to_vec()));
            at = to;
        }
        Some(Frame { kind, fields })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_bad_bodies_are_refused() {
        let f = Frame::new(kind::OPEN)
            .str(tag::URL, "/tmp/x.html")
            .num(tag::WIDTH, 1280)
            .num(200, u64::MAX);
        let b = f.encode();
        assert_eq!(
            u32::from_le_bytes(b[..4].try_into().unwrap()) as usize,
            b.len() - 4
        );
        let g = Frame::decode_body(&b[4..]).unwrap();
        assert_eq!(g, f);
        assert_eq!(g.get_num(tag::WIDTH), Some(1280));
        assert_eq!(g.get_num(200), Some(u64::MAX));
        assert_eq!(Frame::decode_body(&[1]), None);
        assert_eq!(Frame::decode_body(&[1, 0, 7, 5, b'a']), None);
    }
}
