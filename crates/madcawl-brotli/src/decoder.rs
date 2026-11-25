//! The decoder's state and its outer loop.

use crate::bits::Bits;
use crate::dict::MAX_WORD;
use crate::header::{Head, Scratch, Tables};
use crate::ring::Ring;
use crate::{Error, Progress, Status};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Stream,
    Header,
    Metadata(usize),
    Raw,
    Head(Head),
    Command,
    Literals,
    Distance,
    Copy,
    Word,
    /// the last meta-block ended: the rest of its byte must be zero
    End,
    Done,
    Failed(Error),
}

/// Why `run` stopped
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    Next,
    Input,
    Output,
    Done,
}

/// One Brotli stream being decoded
pub struct Decoder {
    pub(crate) br: Bits,
    pub(crate) out: Ring,
    pub(crate) step: Step,
    pub(crate) t: Tables,
    pub(crate) scratch: Scratch,
    pub(crate) limit: usize,
    pub(crate) wbits: u32,
    pub(crate) max_backward: usize,
    pub(crate) is_last: bool,
    /// bytes still to produce in this meta-block
    pub(crate) left: usize,
    /// the last four distances, `dist_at` the next slot
    pub(crate) dist: [usize; 4],
    pub(crate) dist_at: usize,
    pub(crate) insert: usize,
    pub(crate) copy_len: usize,
    pub(crate) implicit_distance: bool,
    pub(crate) copy_dist: usize,
    pub(crate) word: [u8; MAX_WORD],
    pub(crate) word_len: usize,
    /// input bytes taken over all calls
    pub(crate) fed_total: usize,
}

impl Decoder {
    /// A decoder whose total output may be at most `limit` bytes
    pub fn new(limit: usize) -> Self {
        Decoder {
            br: Bits::new(),
            out: Ring::new(),
            step: Step::Stream,
            t: Tables::new(),
            scratch: Scratch::new(),
            limit,
            wbits: 0,
            max_backward: 0,
            is_last: false,
            left: 0,
            dist: [16, 15, 11, 4],
            dist_at: 0,
            insert: 0,
            copy_len: 0,
            implicit_distance: false,
            copy_dist: 0,
            word: [0; MAX_WORD],
            word_len: 0,
            fed_total: 0,
        }
    }

    /// Decodes what `input` allows into `output`. After an error the
    /// decoder returns the same error again.
    pub fn decompress(&mut self, input: &[u8], output: &mut [u8]) -> Result<Progress, Error> {
        let (mut consumed, mut produced) = (0, 0);
        let progress = |consumed, produced, status| Ok(Progress { consumed, produced, status });
        loop {
            if let Step::Failed(e) = self.step {
                return Err(e);
            }
            produced += self.out.flush(&mut output[produced..]);
            if self.step == Step::Done {
                // what is still buffered lies after the end of the stream
                consumed -= self.br.unread().min(consumed);
                let status = if self.out.pending() == 0 { Status::Done } else { Status::NeedOutput };
                return progress(consumed, produced, status);
            }
            let fed = self.br.feed(&input[consumed..]);
            consumed += fed;
            self.fed_total += fed;
            let flow = match self.run() {
                Ok(f) => f,
                Err(e) => {
                    self.step = Step::Failed(e);
                    return Err(e);
                }
            };
            match flow {
                Flow::Input if consumed == input.len() => {
                    produced += self.out.flush(&mut output[produced..]);
                    let status = if self.out.pending() == 0 { Status::NeedInput } else { Status::NeedOutput };
                    return progress(consumed, produced, status);
                }
                Flow::Input if fed == 0 => {
                    // a unit never needs the whole buffer
                    self.step = Step::Failed(Error::Format("unit larger than the input buffer"));
                }
                Flow::Output if produced == output.len() => return progress(consumed, produced, Status::NeedOutput),
                _ => {}
            }
        }
    }

    /// The length of the compressed stream once `decompress` has returned
    /// `Status::Done`, `None` before
    pub fn stream_len(&self) -> Option<usize> {
        let done = self.step == Step::Done && self.out.pending() == 0;
        done.then(|| self.fed_total - self.br.unread())
    }

    fn run(&mut self) -> Result<Flow, Error> {
        loop {
            let flow = match self.step {
                Step::Stream => self.stream_header()?,
                Step::Header => self.meta_header()?,
                Step::Metadata(n) => self.metadata(n),
                Step::Raw => self.raw(),
                Step::Head(h) => self.head(h)?,
                Step::Command => self.command()?,
                Step::Literals => self.literals(),
                Step::Distance => self.distance()?,
                Step::Copy => self.copy(),
                Step::Word => self.put_word(),
                Step::End => self.end()?,
                Step::Done => Flow::Done,
                Step::Failed(e) => return Err(e),
            };
            if flow != Flow::Next {
                return Ok(flow);
            }
        }
    }

    /// Runs `f` as one unit: if the input ran out inside it, everything
    /// it read is put back and the result is `None`
    pub(crate) fn unit<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<Option<T>, Error> {
        let m = self.br.mark();
        let r = f(self);
        if self.br.short {
            self.br.reset(m);
            return Ok(None);
        }
        r.map(Some)
    }

    /// The end of a meta-block
    pub(crate) fn meta_block_done(&mut self) {
        self.step = if self.is_last { Step::End } else { Step::Header };
    }

    fn end(&mut self) -> Result<Flow, Error> {
        if self.br.align() != 0 {
            return Err(Error::Format("nonzero padding at the end"));
        }
        self.step = Step::Done;
        Ok(Flow::Next)
    }
}
