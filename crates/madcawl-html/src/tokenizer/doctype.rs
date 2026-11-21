//! DOCTYPE.

use super::{Go, NUL, REPLACEMENT, Sink, State, Step, Tokenizer, getc, is_space, lower};
use crate::error::ParseError;
use crate::input::{EOF, push_char};

impl Tokenizer {
    fn new_doctype(&mut self) {
        let d = &mut self.doctype;
        d.force_quirks = false;
        for part in [&mut d.name, &mut d.public_id, &mut d.system_id] {
            if let Some(v) = part.as_mut() {
                v.clear();
            }
            *part = None;
        }
    }

    /// The end of the input inside a DOCTYPE.
    fn eof_in_doctype<S: Sink>(&mut self, sink: &mut S) -> Go {
        sink.error(ParseError::EofInDoctype);
        self.doctype.force_quirks = true;
        self.emit_doctype(sink);
        self.emit_eof(sink)
    }

    /// `>` where it closes the DOCTYPE early, with `error`.
    fn doctype_cut_short<S: Sink>(&mut self, sink: &mut S, error: ParseError) -> Go {
        sink.error(error);
        self.doctype.force_quirks = true;
        self.state = State::Data;
        self.emit_doctype(sink);
        Go::On
    }

    fn doctype_done<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.state = State::Data;
        self.emit_doctype(sink);
        Go::On
    }

    fn bogus_doctype_after<S: Sink>(&mut self, sink: &mut S, error: ParseError) -> Go {
        sink.error(error);
        self.doctype.force_quirks = true;
        self.reconsume(State::BogusDoctype)
    }

    pub(super) fn doctype<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => self.switch(State::BeforeDoctypeName),
            0x3E => self.reconsume(State::BeforeDoctypeName),
            EOF => {
                self.new_doctype();
                self.eof_in_doctype(sink)
            }
            _ => {
                sink.error(ParseError::MissingWhitespaceBeforeDoctypeName);
                self.reconsume(State::BeforeDoctypeName)
            }
        }
    }

    pub(super) fn before_doctype_name<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => Go::On,
            0x3E => {
                self.new_doctype();
                self.doctype_cut_short(sink, ParseError::MissingDoctypeName)
            }
            EOF => {
                self.new_doctype();
                self.eof_in_doctype(sink)
            }
            _ => {
                self.new_doctype();
                let c = if c == NUL {
                    sink.error(ParseError::UnexpectedNullCharacter);
                    REPLACEMENT
                } else {
                    lower(c)
                };
                let mut name = Vec::new();
                push_char(&mut name, c);
                self.doctype.name = Some(name);
                self.switch(State::DoctypeName)
            }
        }
    }

    pub(super) fn doctype_name<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => self.switch(State::AfterDoctypeName),
            0x3E => self.doctype_done(sink),
            EOF => self.eof_in_doctype(sink),
            _ => {
                let c = if c == NUL {
                    sink.error(ParseError::UnexpectedNullCharacter);
                    REPLACEMENT
                } else {
                    lower(c)
                };
                push_char(self.doctype.name.get_or_insert_with(Vec::new), c);
                Go::On
            }
        }
    }

    pub(super) fn after_doctype_name<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => Go::On,
            0x3E => self.doctype_done(sink),
            EOF => self.eof_in_doctype(sink),
            _ => {
                self.input.unread();
                let Some(public) = self
                    .input
                    .starts_with(b"PUBLIC", true, &mut |e| sink.error(e))
                else {
                    return Go::Stop(Step::NeedInput);
                };
                if public {
                    self.input.skip(6);
                    return self.switch(State::AfterDoctypePublicKeyword);
                }
                let Some(system) = self
                    .input
                    .starts_with(b"SYSTEM", true, &mut |e| sink.error(e))
                else {
                    return Go::Stop(Step::NeedInput);
                };
                if system {
                    self.input.skip(6);
                    return self.switch(State::AfterDoctypeSystemKeyword);
                }
                sink.error(ParseError::InvalidCharacterSequenceAfterDoctypeName);
                self.doctype.force_quirks = true;
                self.switch(State::BogusDoctype)
            }
        }
    }

    /// Start a public or system identifier, empty.
    fn start_identifier(&mut self, public: bool, quote: u32) -> Go {
        let slot = if public {
            &mut self.doctype.public_id
        } else {
            &mut self.doctype.system_id
        };
        slot.get_or_insert_with(Vec::new).clear();
        let state = match (public, quote) {
            (true, 0x22) => State::DoctypePublicIdentifierDoubleQuoted,
            (true, _) => State::DoctypePublicIdentifierSingleQuoted,
            (false, 0x22) => State::DoctypeSystemIdentifierDoubleQuoted,
            (false, _) => State::DoctypeSystemIdentifierSingleQuoted,
        };
        self.switch(state)
    }

    pub(super) fn after_doctype_keyword<S: Sink>(&mut self, sink: &mut S, public: bool) -> Go {
        let c = getc!(self, sink);
        let (before, missing_space, missing_id, missing_quote) = if public {
            (
                State::BeforeDoctypePublicIdentifier,
                ParseError::MissingWhitespaceAfterDoctypePublicKeyword,
                ParseError::MissingDoctypePublicIdentifier,
                ParseError::MissingQuoteBeforeDoctypePublicIdentifier,
            )
        } else {
            (
                State::BeforeDoctypeSystemIdentifier,
                ParseError::MissingWhitespaceAfterDoctypeSystemKeyword,
                ParseError::MissingDoctypeSystemIdentifier,
                ParseError::MissingQuoteBeforeDoctypeSystemIdentifier,
            )
        };
        match c {
            _ if is_space(c) => self.switch(before),
            0x22 | 0x27 => {
                sink.error(missing_space);
                self.start_identifier(public, c)
            }
            0x3E => self.doctype_cut_short(sink, missing_id),
            EOF => self.eof_in_doctype(sink),
            _ => self.bogus_doctype_after(sink, missing_quote),
        }
    }

    pub(super) fn before_doctype_identifier<S: Sink>(&mut self, sink: &mut S, public: bool) -> Go {
        let c = getc!(self, sink);
        let (missing_id, missing_quote) = if public {
            (
                ParseError::MissingDoctypePublicIdentifier,
                ParseError::MissingQuoteBeforeDoctypePublicIdentifier,
            )
        } else {
            (
                ParseError::MissingDoctypeSystemIdentifier,
                ParseError::MissingQuoteBeforeDoctypeSystemIdentifier,
            )
        };
        match c {
            _ if is_space(c) => Go::On,
            0x22 | 0x27 => self.start_identifier(public, c),
            0x3E => self.doctype_cut_short(sink, missing_id),
            EOF => self.eof_in_doctype(sink),
            _ => self.bogus_doctype_after(sink, missing_quote),
        }
    }

    pub(super) fn doctype_identifier<S: Sink>(
        &mut self,
        sink: &mut S,
        public: bool,
        quote: u8,
    ) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if c == u32::from(quote) => self.switch(if public {
                State::AfterDoctypePublicIdentifier
            } else {
                State::AfterDoctypeSystemIdentifier
            }),
            0x3E => self.doctype_cut_short(
                sink,
                if public {
                    ParseError::AbruptDoctypePublicIdentifier
                } else {
                    ParseError::AbruptDoctypeSystemIdentifier
                },
            ),
            EOF => self.eof_in_doctype(sink),
            _ => {
                let c = if c == NUL {
                    sink.error(ParseError::UnexpectedNullCharacter);
                    REPLACEMENT
                } else {
                    c
                };
                let slot = if public {
                    &mut self.doctype.public_id
                } else {
                    &mut self.doctype.system_id
                };
                push_char(slot.get_or_insert_with(Vec::new), c);
                Go::On
            }
        }
    }

    pub(super) fn after_doctype_public_identifier<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => self.switch(State::BetweenDoctypePublicAndSystemIdentifiers),
            0x3E => self.doctype_done(sink),
            0x22 | 0x27 => {
                sink.error(ParseError::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers);
                self.start_identifier(false, c)
            }
            EOF => self.eof_in_doctype(sink),
            _ => self
                .bogus_doctype_after(sink, ParseError::MissingQuoteBeforeDoctypeSystemIdentifier),
        }
    }

    pub(super) fn between_doctype_identifiers<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => Go::On,
            0x3E => self.doctype_done(sink),
            0x22 | 0x27 => self.start_identifier(false, c),
            EOF => self.eof_in_doctype(sink),
            _ => self
                .bogus_doctype_after(sink, ParseError::MissingQuoteBeforeDoctypeSystemIdentifier),
        }
    }

    pub(super) fn after_doctype_system_identifier<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => Go::On,
            0x3E => self.doctype_done(sink),
            EOF => self.eof_in_doctype(sink),
            _ => {
                // not a quirk: the identifiers so far stand
                sink.error(ParseError::UnexpectedCharacterAfterDoctypeSystemIdentifier);
                self.reconsume(State::BogusDoctype)
            }
        }
    }

    pub(super) fn bogus_doctype<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x3E => self.doctype_done(sink),
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                Go::On
            }
            EOF => {
                self.emit_doctype(sink);
                self.emit_eof(sink)
            }
            _ => Go::On,
        }
    }
}
