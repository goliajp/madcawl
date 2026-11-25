//! The stream header (section 9.1), meta-block headers up to the
//! compressed part (9.2), metadata and uncompressed meta-blocks.

use crate::Error;
use crate::decoder::{Decoder, Flow, Step};
use crate::header::Head;

impl Decoder {
    pub(crate) fn stream_header(&mut self) -> Result<Flow, Error> {
        let Some(wbits) = self.unit(|d| {
            let br = &mut d.br;
            if br.take(1) == 0 {
                return Ok(16);
            }
            let n = br.take(3);
            if n != 0 {
                return Ok(17 + n);
            }
            match br.take(3) {
                0 => Ok(17),
                1 => Err(Error::Format("large window streams are not supported")),
                m => Ok(8 + m),
            }
        })?
        else {
            return Ok(Flow::Input);
        };
        self.wbits = wbits;
        self.max_backward = (1 << wbits) - 16;
        self.step = Step::Header;
        Ok(Flow::Next)
    }

    pub(crate) fn meta_header(&mut self) -> Result<Flow, Error> {
        let Some(h) = self.unit(read_meta_header)? else {
            return Ok(Flow::Input);
        };
        self.is_last = h.last;
        self.step = match h.kind {
            MetaKind::Empty => Step::End,
            MetaKind::Metadata(n) => Step::Metadata(n),
            MetaKind::Data { len, raw } => {
                if len > self.limit - self.out.written {
                    return Err(Error::TooLarge);
                }
                if !self.out.is_allocated() {
                    // the window need not be larger than all the output can be
                    let window = 1usize << self.wbits;
                    let bound = if h.last { len } else { self.limit };
                    self.out.allocate(if bound < window {
                        bound.max(1024).next_power_of_two().min(window)
                    } else {
                        window
                    });
                }
                self.left = len;
                if raw { Step::Raw } else { Step::Head(Head::Types(0)) }
            }
        };
        Ok(Flow::Next)
    }

    pub(crate) fn metadata(&mut self, n: usize) -> Flow {
        if n == 0 {
            self.step = if self.is_last { Step::End } else { Step::Header };
            return Flow::Next;
        }
        let k = self.br.skip_bytes(n);
        if k == 0 {
            return Flow::Input;
        }
        self.step = Step::Metadata(n - k);
        Flow::Next
    }

    pub(crate) fn raw(&mut self) -> Flow {
        while self.left > 0 {
            let want = self.left;
            let space = self.out.space();
            if space.is_empty() {
                return Flow::Output;
            }
            let n = space.len().min(want);
            let k = self.br.bytes(&mut space[..n]);
            if k == 0 {
                return Flow::Input;
            }
            self.out.commit(k);
            self.left -= k;
        }
        self.step = Step::Header;
        Flow::Next
    }
}

struct MetaHeader {
    last: bool,
    kind: MetaKind,
}

enum MetaKind {
    Empty,
    Metadata(usize),
    Data { len: usize, raw: bool },
}

fn read_meta_header(d: &mut Decoder) -> Result<MetaHeader, Error> {
    let br = &mut d.br;
    let last = br.take(1) == 1;
    if last && br.take(1) == 1 {
        return Ok(MetaHeader { last, kind: MetaKind::Empty });
    }
    let nibbles = br.take(2);
    if nibbles == 3 {
        if br.take(1) != 0 {
            return Err(Error::Format("reserved bit set"));
        }
        let bytes = br.take(2);
        let mut len = 0;
        if bytes > 0 {
            let v = br.take(8 * bytes);
            if bytes > 1 && v >> (8 * (bytes - 1)) == 0 {
                return Err(Error::Format("metadata length has a zero last byte"));
            }
            len = v as usize + 1;
        }
        if br.align() != 0 {
            return Err(Error::Format("nonzero padding before metadata"));
        }
        return Ok(MetaHeader { last, kind: MetaKind::Metadata(len) });
    }
    let nibbles = nibbles + 4;
    let v = br.take(4 * nibbles);
    if nibbles > 4 && v >> (4 * (nibbles - 1)) == 0 {
        return Err(Error::Format("meta-block length has a zero last nibble"));
    }
    let raw = !last && br.take(1) == 1;
    if raw && br.align() != 0 {
        return Err(Error::Format("nonzero padding before uncompressed data"));
    }
    Ok(MetaHeader { last, kind: MetaKind::Data { len: v as usize + 1, raw } })
}
