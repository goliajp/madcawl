//! Script data: the `<!--` escapes, where `</script>` inside a nested
//! `<script>` in a comment does not end the script.

use super::{Go, NUL, REPLACEMENT, Sink, State, Tokenizer, getc, is_alpha, is_space, lower};
use crate::error::ParseError;
use crate::input::{EOF, Stops, stops};

static ESCAPED_STOPS: Stops = stops(b"-<");

impl Tokenizer {
    pub(super) fn script_data_less_than<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2F => {
                self.temp.clear();
                self.switch(State::ScriptDataEndTagOpen)
            }
            0x21 => {
                Self::emit_bytes(sink, b"<!");
                self.switch(State::ScriptDataEscapeStart)
            }
            _ => {
                self.emit_char(sink, 0x3C);
                self.reconsume(State::ScriptData)
            }
        }
    }

    pub(super) fn script_escape_start<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if c == 0x2D {
            self.emit_char(sink, 0x2D);
            return self.switch(State::ScriptDataEscapeStartDash);
        }
        self.reconsume(State::ScriptData)
    }

    pub(super) fn script_escape_start_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if c == 0x2D {
            self.emit_char(sink, 0x2D);
            return self.switch(State::ScriptDataEscapedDashDash);
        }
        self.reconsume(State::ScriptData)
    }

    pub(super) fn script_escaped<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.text_run(sink, &ESCAPED_STOPS);
        let c = getc!(self, sink);
        match c {
            0x2D => {
                self.emit_char(sink, 0x2D);
                self.switch(State::ScriptDataEscapedDash)
            }
            0x3C => self.switch(State::ScriptDataEscapedLessThanSign),
            _ => self.script_escaped_other(sink, c, State::ScriptDataEscaped),
        }
    }

    pub(super) fn script_escaped_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2D => {
                self.emit_char(sink, 0x2D);
                self.switch(State::ScriptDataEscapedDashDash)
            }
            0x3C => self.switch(State::ScriptDataEscapedLessThanSign),
            _ => self.script_escaped_other(sink, c, State::ScriptDataEscaped),
        }
    }

    pub(super) fn script_escaped_dash_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2D => {
                self.emit_char(sink, 0x2D);
                Go::On
            }
            0x3C => self.switch(State::ScriptDataEscapedLessThanSign),
            0x3E => {
                self.emit_char(sink, 0x3E);
                self.switch(State::ScriptData)
            }
            _ => self.script_escaped_other(sink, c, State::ScriptDataEscaped),
        }
    }

    /// NUL, the end, and anything else in the escaped and double
    /// escaped states: emitted, and back to `back`.
    fn script_escaped_other<S: Sink>(&mut self, sink: &mut S, c: u32, back: State) -> Go {
        match c {
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                self.emit_char(sink, REPLACEMENT);
                self.switch(back)
            }
            EOF => {
                sink.error(ParseError::EofInScriptHtmlCommentLikeText);
                self.emit_eof(sink)
            }
            _ => {
                self.emit_char(sink, c);
                self.switch(back)
            }
        }
    }

    pub(super) fn script_escaped_less_than<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if c == 0x2F {
            self.temp.clear();
            return self.switch(State::ScriptDataEscapedEndTagOpen);
        }
        self.emit_char(sink, 0x3C);
        if is_alpha(c) {
            self.temp.clear();
            return self.reconsume(State::ScriptDataDoubleEscapeStart);
        }
        self.reconsume(State::ScriptDataEscaped)
    }

    /// The double escape start and end states: `<script` inside an
    /// escape starts a double escape, `</script` ends it.
    pub(super) fn script_double_escape_edge<S: Sink>(&mut self, sink: &mut S, start: bool) -> Go {
        let (inside, outside) = if start {
            (State::ScriptDataDoubleEscaped, State::ScriptDataEscaped)
        } else {
            (State::ScriptDataEscaped, State::ScriptDataDoubleEscaped)
        };
        let c = getc!(self, sink);
        if is_space(c) || c == 0x2F || c == 0x3E {
            self.emit_char(sink, c);
            let next = if self.temp == b"script" {
                inside
            } else {
                outside
            };
            return self.switch(next);
        }
        if is_alpha(c) {
            self.temp.push(lower(c) as u8);
            self.emit_char(sink, c);
            return Go::On;
        }
        self.reconsume(outside)
    }

    pub(super) fn script_double_escaped<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.text_run(sink, &ESCAPED_STOPS);
        let c = getc!(self, sink);
        match c {
            0x2D => {
                self.emit_char(sink, 0x2D);
                self.switch(State::ScriptDataDoubleEscapedDash)
            }
            0x3C => {
                self.emit_char(sink, 0x3C);
                self.switch(State::ScriptDataDoubleEscapedLessThanSign)
            }
            _ => self.script_escaped_other(sink, c, State::ScriptDataDoubleEscaped),
        }
    }

    pub(super) fn script_double_escaped_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2D => {
                self.emit_char(sink, 0x2D);
                self.switch(State::ScriptDataDoubleEscapedDashDash)
            }
            0x3C => {
                self.emit_char(sink, 0x3C);
                self.switch(State::ScriptDataDoubleEscapedLessThanSign)
            }
            _ => self.script_escaped_other(sink, c, State::ScriptDataDoubleEscaped),
        }
    }

    pub(super) fn script_double_escaped_dash_dash<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x2D => {
                self.emit_char(sink, 0x2D);
                Go::On
            }
            0x3C => {
                self.emit_char(sink, 0x3C);
                self.switch(State::ScriptDataDoubleEscapedLessThanSign)
            }
            0x3E => {
                self.emit_char(sink, 0x3E);
                self.switch(State::ScriptData)
            }
            _ => self.script_escaped_other(sink, c, State::ScriptDataDoubleEscaped),
        }
    }

    pub(super) fn script_double_escaped_less_than<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        if c == 0x2F {
            self.temp.clear();
            self.emit_char(sink, 0x2F);
            return self.switch(State::ScriptDataDoubleEscapeEnd);
        }
        self.reconsume(State::ScriptDataDoubleEscaped)
    }
}
