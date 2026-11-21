//! Text states: data, RCDATA, RAWTEXT, script data, PLAINTEXT, and the
//! end tags that close RCDATA, RAWTEXT and script data.

use super::{Go, NUL, REPLACEMENT, Sink, State, Tokenizer, getc, is_alpha, is_space, lower};
use crate::error::ParseError;
use crate::input::{EOF, stops};
use crate::token::Token;

static DATA_STOPS: [bool; 128] = stops(b"<&");
static RAWTEXT_STOPS: [bool; 128] = stops(b"<");
static PLAINTEXT_STOPS: [bool; 128] = stops(b"");

impl Tokenizer {
    /// Emit the run of plain text that starts here, uncopied.
    #[inline]
    pub(super) fn text_run<S: Sink>(&mut self, sink: &mut S, stops: &[bool; 128]) {
        let (a, b) = self.input.run(stops);
        if b > a {
            sink.token(Token::Chars(self.input.bytes(a, b)));
        }
    }

    pub(super) fn data<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.text_run(sink, &DATA_STOPS);
        let c = getc!(self, sink);
        match c {
            0x26 => {
                self.return_state = State::Data;
                self.switch(State::CharacterReference)
            }
            0x3C => self.switch(State::TagOpen),
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                self.emit_char(sink, NUL);
                Go::On
            }
            EOF => self.emit_eof(sink),
            _ => {
                self.emit_char(sink, c);
                Go::On
            }
        }
    }

    pub(super) fn rcdata<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.text(
            sink,
            State::Rcdata,
            &DATA_STOPS,
            true,
            Some(State::RcdataLessThanSign),
        )
    }

    pub(super) fn rawtext<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.text(
            sink,
            State::Rawtext,
            &RAWTEXT_STOPS,
            false,
            Some(State::RawtextLessThanSign),
        )
    }

    pub(super) fn script_data<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.text(
            sink,
            State::ScriptData,
            &RAWTEXT_STOPS,
            false,
            Some(State::ScriptDataLessThanSign),
        )
    }

    pub(super) fn plaintext<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.text(sink, State::Plaintext, &PLAINTEXT_STOPS, false, None)
    }

    /// The text states other than data: NUL becomes U+FFFD.
    fn text<S: Sink>(
        &mut self,
        sink: &mut S,
        here: State,
        stops: &[bool; 128],
        references: bool,
        less_than: Option<State>,
    ) -> Go {
        self.text_run(sink, stops);
        let c = getc!(self, sink);
        match c {
            0x26 if references => {
                self.return_state = here;
                self.switch(State::CharacterReference)
            }
            0x3C if less_than.is_some() => self.switch(less_than.expect("guarded")),
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                self.emit_char(sink, REPLACEMENT);
                Go::On
            }
            EOF => self.emit_eof(sink),
            _ => {
                self.emit_char(sink, c);
                Go::On
            }
        }
    }

    /// RCDATA and RAWTEXT less-than sign states.
    pub(super) fn text_less_than<S: Sink>(
        &mut self,
        sink: &mut S,
        text: State,
        end_tag_open: State,
    ) -> Go {
        let c = getc!(self, sink);
        if c == 0x2F {
            self.temp.clear();
            return self.switch(end_tag_open);
        }
        self.emit_char(sink, 0x3C);
        self.reconsume(text)
    }

    /// The end tag open states of RCDATA, RAWTEXT, script data and
    /// escaped script data.
    pub(super) fn text_end_tag_open<S: Sink>(
        &mut self,
        sink: &mut S,
        text: State,
        end_tag_name: State,
    ) -> Go {
        let c = getc!(self, sink);
        if is_alpha(c) {
            self.new_tag(true);
            return self.reconsume(end_tag_name);
        }
        Self::emit_bytes(sink, b"</");
        self.reconsume(text)
    }

    /// The end tag name states of the same four: only an end tag for
    /// the element the text is in ends the text.
    pub(super) fn text_end_tag_name<S: Sink>(&mut self, sink: &mut S, text: State) -> Go {
        let c = getc!(self, sink);
        if is_space(c) && self.appropriate_end_tag() {
            return self.switch(State::BeforeAttributeName);
        }
        if c == 0x2F && self.appropriate_end_tag() {
            return self.switch(State::SelfClosingStartTag);
        }
        if c == 0x3E && self.appropriate_end_tag() {
            self.state = State::Data;
            return self.emit_tag(sink);
        }
        if is_alpha(c) {
            self.tag.name.push(lower(c) as u8);
            self.temp.push(c as u8);
            return Go::On;
        }
        Self::emit_bytes(sink, b"</");
        let temp = std::mem::take(&mut self.temp);
        Self::emit_bytes(sink, &temp);
        self.temp = temp;
        self.reconsume(text)
    }
}
