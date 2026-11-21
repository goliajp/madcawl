//! Processing instructions, `<?target data?>`. The temporary buffer
//! holds the target while it is read.

use super::{Go, Sink, State, Tokenizer, getc, is_alpha, is_space};
use crate::error::ParseError;
use crate::input::{EOF, push_char};
use crate::token::Token;

impl Tokenizer {
    /// A target that is not one becomes a bogus comment, `?` first.
    fn pi_to_comment(&mut self) -> Go {
        self.comment.clear();
        self.comment.push(b'?');
        self.comment.extend_from_slice(&self.temp);
        self.reconsume(State::BogusComment)
    }

    fn eof_in_pi<S: Sink>(&mut self, sink: &mut S) -> Go {
        sink.error(ParseError::EofInProcessingInstruction);
        self.emit_eof(sink)
    }

    fn emit_pi<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.state = State::Data;
        sink.token(Token::ProcessingInstruction {
            target: &self.temp,
            data: &self.pi_data,
        });
        Go::On
    }

    pub(super) fn pi_open<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_alpha(c) || c == 0x5F => self.reconsume(State::ProcessingInstructionTarget),
            EOF => self.eof_in_pi(sink),
            _ => {
                sink.error(ParseError::InvalidFirstCharacterOfProcessingInstructionTarget);
                self.pi_to_comment()
            }
        }
    }

    pub(super) fn pi_target<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) || c == 0x3F || c == 0x3E => {
                if self.temp.eq_ignore_ascii_case(b"xml")
                    || self.temp.eq_ignore_ascii_case(b"xml-stylesheet")
                {
                    sink.error(ParseError::DisallowedProcessingInstructionTarget);
                    return self.pi_to_comment();
                }
                self.pi_data.clear();
                self.reconsume(State::AfterProcessingInstructionTarget)
            }
            0x30..=0x39 | 0x2D | 0x5F => {
                self.temp.push(c as u8);
                Go::On
            }
            _ if is_alpha(c) => {
                self.temp.push(c as u8);
                Go::On
            }
            EOF => self.eof_in_pi(sink),
            _ => {
                sink.error(ParseError::InvalidProcessingInstructionTarget);
                self.pi_to_comment()
            }
        }
    }

    pub(super) fn after_pi_target<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if is_space(c) {
            return Go::On;
        }
        self.reconsume(State::ProcessingInstructionData)
    }

    pub(super) fn pi_data<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x3F => self.switch(State::ProcessingInstructionQuestionable),
            0x3E => self.emit_pi(sink),
            EOF => self.eof_in_pi(sink),
            _ => {
                push_char(&mut self.pi_data, c);
                Go::On
            }
        }
    }

    pub(super) fn pi_questionable<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x3E => self.emit_pi(sink),
            EOF => self.eof_in_pi(sink),
            _ => {
                self.pi_data.push(b'?');
                self.reconsume(State::ProcessingInstructionData)
            }
        }
    }
}
