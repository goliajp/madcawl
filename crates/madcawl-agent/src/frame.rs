//! The agent protocol's frames: a length (u32, little-endian, of what
//! follows it), a kind (u16), then fields, each a tag byte, a LEB128
//! length and that many bytes. A reader skips frames of kinds and fields
//! of tags it does not know, so either side can add them later.

/// The longest frame a reader takes; a longer length is an error, never
/// an allocation.
pub const MAX_FRAME: usize = 16 << 20;

/// The most fields a frame may have; more is an error, so a frame of
/// empty fields cannot make a reader allocate for millions of them.
pub const MAX_FIELDS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: u16,
    pub fields: Vec<(u8, Vec<u8>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// More bytes are needed
    Short,
    TooLong,
    /// A field runs past the frame
    Malformed,
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
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

    pub fn field(&self, tag: u8) -> Option<&[u8]> {
        self.fields
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| v.as_slice())
    }

    pub fn get_str(&self, tag: u8) -> Option<&str> {
        std::str::from_utf8(self.field(tag)?).ok()
    }

    pub fn get_num(&self, tag: u8) -> Option<u64> {
        let b = self.field(tag)?;
        get_varint(b).filter(|&(_, n)| n == b.len()).map(|(v, _)| v)
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
        let Some(head) = b.get(..4) else {
            return Err(FrameError::Short);
        };
        let len = u32::from_le_bytes(head.try_into().expect("four bytes")) as usize;
        if len > MAX_FRAME {
            return Err(FrameError::TooLong);
        }
        if len < 2 {
            return Err(FrameError::Malformed);
        }
        let body = b.get(4..4 + len).ok_or(FrameError::Short)?;
        let kind = u16::from_le_bytes([body[0], body[1]]);
        let mut fields = Vec::new();
        let mut at = 2;
        while at < body.len() {
            if fields.len() == MAX_FIELDS {
                return Err(FrameError::Malformed);
            }
            let tag = body[at];
            let (n, used) = get_varint(&body[at + 1..]).ok_or(FrameError::Malformed)?;
            let from = at + 1 + used;
            let to = from
                .checked_add(usize::try_from(n).map_err(|_| FrameError::Malformed)?)
                .filter(|&t| t <= body.len())
                .ok_or(FrameError::Malformed)?;
            fields.push((tag, body[from..to].to_vec()));
            at = to;
        }
        Ok((Frame { kind, fields }, 4 + len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_unknown_parts_are_carried_not_refused() {
        let f = Frame::new(0x7777)
            .num(1, 300)
            .str(2, "snapshot")
            .bytes(200, &[1, 2, 3]);
        let mut b = Vec::new();
        f.encode(&mut b);
        Frame::new(10).encode(&mut b);
        let (g, used) = Frame::decode(&b).unwrap();
        assert_eq!(g, f);
        assert_eq!(g.get_num(1), Some(300));
        assert_eq!(g.get_str(2), Some("snapshot"));
        assert_eq!(g.get_num(9), None);
        let (h, _) = Frame::decode(&b[used..]).unwrap();
        assert_eq!(h.kind, 10);
    }

    #[test]
    fn bad_lengths_are_errors_not_panics() {
        assert_eq!(Frame::decode(&[1, 0]), Err(FrameError::Short));
        assert_eq!(
            Frame::decode(&[0xff, 0xff, 0xff, 0xff]),
            Err(FrameError::TooLong)
        );
        assert_eq!(Frame::decode(&[5, 0, 0, 0, 1, 0]), Err(FrameError::Short));
        // a field claiming more bytes than the frame has
        assert_eq!(
            Frame::decode(&[5, 0, 0, 0, 1, 0, 3, 9, 0]),
            Err(FrameError::Malformed)
        );
        // a frame of the largest size, all empty fields
        let mut hostile = ((MAX_FRAME) as u32).to_le_bytes().to_vec();
        hostile.extend_from_slice(&[1, 0]);
        hostile.resize(4 + MAX_FRAME, 0);
        assert_eq!(Frame::decode(&hostile), Err(FrameError::Malformed));
        for n in 0..64u8 {
            let junk: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37) ^ n).collect();
            let _ = Frame::decode(&junk);
        }
    }
}
