//! The `br` layer: `madcawl-brotli` behind the same cap and end rules as
//! the inflate layers.

use super::{Emit, Stage, State};
use crate::error::NetError;
use madcawl_brotli::{Decoder, Error, Status};

impl Stage {
    pub(super) fn brotli(cap: usize) -> Self {
        Stage {
            // never borrowed: the brotli decoder keeps its own state
            state: State { mem: Vec::new(), borrowed: false },
            format: None,
            pending: Vec::new(),
            done: false,
            chunk: vec![0u8; 32 * 1024],
            produced: 0,
            cap,
            brotli: Some(Box::new(Decoder::new(cap))),
        }
    }

    pub(super) fn run_brotli(&mut self, input: &[u8], finish: bool, emit: &mut Emit<'_>) -> Result<(), NetError> {
        let Stage { brotli, chunk, produced, cap, done, .. } = self;
        let Some(d) = brotli else { return Ok(()) };
        let mut at = 0;
        loop {
            let p = d.decompress(&input[at..], chunk).map_err(|e| match e {
                Error::TooLarge => NetError::TooLarge("decoded body"),
                e => NetError::Decode(e.to_string()),
            })?;
            at += p.consumed;
            *produced += p.produced;
            if *produced > *cap {
                return Err(NetError::TooLarge("decoded body"));
            }
            if p.produced > 0 {
                emit(&chunk[..p.produced])?;
            }
            match p.status {
                Status::Done => {
                    *done = true;
                    // the window can be large; it is not needed any more
                    *brotli = None;
                    return Ok(());
                }
                Status::NeedOutput => {}
                Status::NeedInput if finish => return Err(NetError::Decode("truncated stream".into())),
                Status::NeedInput => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Coding, Decoder, codings, decode};
    use crate::error::NetError;

    /// "hello, hello, hello" compressed with brotli
    const BR: [u8; 22] = [
        0xa1, 0x90, 0x00, 0xc0, 0x6f, 0xa4, 0x74, 0xf3, 0x26, 0x4a, 0x90, 0x32, 0xb4, 0x64, 0x63, 0x79, 0x8b, 0x78,
        0x22, 0x81, 0x8e, 0x03,
    ];

    /// the same text gzipped, then compressed with brotli
    const GZIP_BR: [u8; 34] = [
        0x21, 0x70, 0x00, 0x04, 0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xcb, 0x48, 0xcd, 0xc9,
        0xc9, 0xd7, 0x51, 0xc8, 0x40, 0xa2, 0x00, 0x9f, 0xa1, 0xca, 0x09, 0x13, 0x00, 0x00, 0x00, 0x03,
    ];

    const TEXT: &[u8] = b"hello, hello, hello";

    fn bytewise(body: &[u8], codings: &[Coding], cap: usize) -> Result<Vec<u8>, NetError> {
        let mut d = Decoder::new(codings, cap);
        let mut out = Vec::new();
        for b in body {
            d.push(std::slice::from_ref(b), &mut |x| {
                out.extend_from_slice(x);
                Ok(())
            })?;
        }
        d.finish(&mut |x| {
            out.extend_from_slice(x);
            Ok(())
        })?;
        Ok(out)
    }

    #[test]
    fn br_decodes_alone_and_after_gzip() {
        assert_eq!(codings(b"br").unwrap(), [Coding::Brotli]);
        assert_eq!(codings(b"gzip, BR").unwrap(), [Coding::Gzip, Coding::Brotli]);
        assert_eq!(decode(&BR, &[Coding::Brotli], 100).unwrap(), TEXT);
        assert_eq!(bytewise(&BR, &[Coding::Brotli], 100).unwrap(), TEXT);
        let chain = [Coding::Gzip, Coding::Brotli];
        assert_eq!(decode(&GZIP_BR, &chain, 100).unwrap(), TEXT);
        assert_eq!(bytewise(&GZIP_BR, &chain, 100).unwrap(), TEXT);
    }

    #[test]
    fn br_keeps_the_cap_and_rejects_a_cut_body() {
        assert_eq!(decode(&BR, &[Coding::Brotli], TEXT.len() - 1), Err(NetError::TooLarge("decoded body")));
        // the gzip layer inside counts against the cap as well
        assert_eq!(
            decode(&GZIP_BR, &[Coding::Gzip, Coding::Brotli], TEXT.len()),
            Err(NetError::TooLarge("decoded body"))
        );
        assert!(matches!(decode(&BR[..BR.len() - 1], &[Coding::Brotli], 100), Err(NetError::Decode(_))));
        assert!(matches!(decode(&[0x11], &[Coding::Brotli], 100), Err(NetError::Decode(_))));
    }
}
