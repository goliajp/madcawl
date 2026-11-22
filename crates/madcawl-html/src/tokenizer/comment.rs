//! Comments, the markup declaration that opens them, and CDATA.

use super::{Go, NUL, REPLACEMENT, Sink, State, Step, Tokenizer, getc};
use crate::error::ParseError;
use crate::input::{EOF, Stops, push_char, stops};

static COMMENT_STOPS: Stops = stops(b"<-");
static BOGUS_STOPS: Stops = stops(b">");
static CDATA_STOPS: Stops = {
    let mut t = stops(b"]");
    // NUL and the controls pass through CDATA as they are, but they
    // still go through `next`, which reports the controls
    t[0] = true;
    t
};

impl Tokenizer {
    pub(super) fn bogus_comment<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.comment_run(&BOGUS_STOPS);
        let c = getc!(self, sink);
        match c {
            0x3E => {
                self.state = State::Data;
                self.emit_comment(sink);
                Go::On
            }
            EOF => {
                self.emit_comment(sink);
                self.emit_eof(sink)
            }
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                push_char(&mut self.comment, REPLACEMENT);
                Go::On
            }
            _ => {
                push_char(&mut self.comment, c);
                Go::On
            }
        }
    }

    fn comment_run(&mut self, stops: &Stops) {
        let (a, b) = self.input.run(stops);
        if b > a {
            self.comment.extend_from_slice(self.input.bytes(a, b));
        }
    }

    pub(super) fn markup_declaration_open<S: Sink>(&mut self, sink: &mut S) -> Go {
        let Some(dashes) = self.input.starts_with(b"--", false, &mut |e| sink.error(e)) else {
            return Go::Stop(Step::NeedInput);
        };
        if dashes {
            self.input.skip(2);
            self.comment.clear();
            return self.switch(State::CommentStart);
        }
        let Some(doctype) = self
            .input
            .starts_with(b"DOCTYPE", true, &mut |e| sink.error(e))
        else {
            return Go::Stop(Step::NeedInput);
        };
        if doctype {
            self.input.skip(7);
            return self.switch(State::Doctype);
        }
        let Some(cdata) = self
            .input
            .starts_with(b"[CDATA[", false, &mut |e| sink.error(e))
        else {
            return Go::Stop(Step::NeedInput);
        };
        if cdata {
            self.input.skip(7);
            if sink.in_foreign_content() {
                return self.switch(State::CdataSection);
            }
            sink.error(ParseError::CdataInHtmlContent);
            self.comment.clear();
            self.comment.extend_from_slice(b"[CDATA[");
            return self.switch(State::BogusComment);
        }
        sink.error(ParseError::IncorrectlyOpenedComment);
        self.comment.clear();
        self.switch(State::BogusComment)
    }

    pub(super) fn comment_start<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2D => self.switch(State::CommentStartDash),
            0x3E => {
                sink.error(ParseError::AbruptClosingOfEmptyComment);
                self.state = State::Data;
                self.emit_comment(sink);
                Go::On
            }
            _ => self.reconsume(State::Comment),
        }
    }

    pub(super) fn comment_start_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2D => self.switch(State::CommentEnd),
            0x3E => {
                sink.error(ParseError::AbruptClosingOfEmptyComment);
                self.state = State::Data;
                self.emit_comment(sink);
                Go::On
            }
            EOF => self.eof_in_comment(sink),
            _ => {
                self.comment.push(b'-');
                self.reconsume(State::Comment)
            }
        }
    }

    fn eof_in_comment<S: Sink>(&mut self, sink: &mut S) -> Go {
        sink.error(ParseError::EofInComment);
        self.emit_comment(sink);
        self.emit_eof(sink)
    }

    pub(super) fn comment<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.comment_run(&COMMENT_STOPS);
        let c = getc!(self, sink);
        match c {
            0x3C => {
                self.comment.push(b'<');
                self.switch(State::CommentLessThanSign)
            }
            0x2D => self.switch(State::CommentEndDash),
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                push_char(&mut self.comment, REPLACEMENT);
                Go::On
            }
            EOF => self.eof_in_comment(sink),
            _ => {
                push_char(&mut self.comment, c);
                Go::On
            }
        }
    }

    pub(super) fn comment_less_than<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x21 => {
                self.comment.push(b'!');
                self.switch(State::CommentLessThanSignBang)
            }
            0x3C => {
                self.comment.push(b'<');
                Go::On
            }
            _ => self.reconsume(State::Comment),
        }
    }

    pub(super) fn comment_less_than_bang<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if c == 0x2D {
            return self.switch(State::CommentLessThanSignBangDash);
        }
        self.reconsume(State::Comment)
    }

    pub(super) fn comment_less_than_bang_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if c == 0x2D {
            return self.switch(State::CommentLessThanSignBangDashDash);
        }
        self.reconsume(State::CommentEndDash)
    }

    pub(super) fn comment_less_than_bang_dash_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if c != 0x3E && c != EOF {
            sink.error(ParseError::NestedComment);
        }
        self.reconsume(State::CommentEnd)
    }

    pub(super) fn comment_end_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2D => self.switch(State::CommentEnd),
            EOF => self.eof_in_comment(sink),
            _ => {
                self.comment.push(b'-');
                self.reconsume(State::Comment)
            }
        }
    }

    pub(super) fn comment_end<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x3E => {
                self.state = State::Data;
                self.emit_comment(sink);
                Go::On
            }
            0x21 => self.switch(State::CommentEndBang),
            0x2D => {
                self.comment.push(b'-');
                Go::On
            }
            EOF => self.eof_in_comment(sink),
            _ => {
                self.comment.extend_from_slice(b"--");
                self.reconsume(State::Comment)
            }
        }
    }

    pub(super) fn comment_end_bang<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2D => {
                self.comment.extend_from_slice(b"--!");
                self.switch(State::CommentEndDash)
            }
            0x3E => {
                sink.error(ParseError::IncorrectlyClosedComment);
                self.state = State::Data;
                self.emit_comment(sink);
                Go::On
            }
            EOF => self.eof_in_comment(sink),
            _ => {
                self.comment.extend_from_slice(b"--!");
                self.reconsume(State::Comment)
            }
        }
    }

    pub(super) fn cdata_section<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.text_run(sink, &CDATA_STOPS);
        let c = getc!(self, sink);
        match c {
            0x5D => self.switch(State::CdataSectionBracket),
            EOF => {
                sink.error(ParseError::EofInCdata);
                self.emit_eof(sink)
            }
            _ => {
                self.emit_char(sink, c);
                Go::On
            }
        }
    }

    pub(super) fn cdata_section_bracket<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if c == 0x5D {
            return self.switch(State::CdataSectionEnd);
        }
        self.emit_char(sink, 0x5D);
        self.reconsume(State::CdataSection)
    }

    pub(super) fn cdata_section_end<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x5D => {
                self.emit_char(sink, 0x5D);
                Go::On
            }
            0x3E => self.switch(State::Data),
            _ => {
                Self::emit_bytes(sink, b"]]");
                self.reconsume(State::CdataSection)
            }
        }
    }
}
