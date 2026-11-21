//! Tags and attributes.

use super::{Go, NUL, REPLACEMENT, Sink, State, Tokenizer, getc, is_alpha, is_space, lower};
use crate::error::ParseError;
use crate::input::{EOF, push_char, stops};

/// Bytes a tag or attribute name state does more with than append.
static NAME_STOPS: [bool; 128] = {
    let mut t = stops(b"\t\n\x0c />=\"'<");
    let mut c = b'A';
    while c <= b'Z' {
        t[c as usize] = true;
        c += 1;
    }
    t
};
static DOUBLE_QUOTED_STOPS: [bool; 128] = stops(b"\"&");
static SINGLE_QUOTED_STOPS: [bool; 128] = stops(b"'&");
static UNQUOTED_STOPS: [bool; 128] = stops(b"\t\n\x0c >&\"'<=`");

impl Tokenizer {
    pub(super) fn tag_open<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x21 => self.switch(State::MarkupDeclarationOpen),
            0x2F => self.switch(State::EndTagOpen),
            _ if is_alpha(c) => {
                self.new_tag(false);
                self.reconsume(State::TagName)
            }
            0x3F => {
                self.temp.clear();
                self.switch(State::ProcessingInstructionOpen)
            }
            EOF => {
                sink.error(ParseError::EofBeforeTagName);
                self.emit_char(sink, 0x3C);
                self.emit_eof(sink)
            }
            _ => {
                sink.error(ParseError::InvalidFirstCharacterOfTagName);
                self.emit_char(sink, 0x3C);
                self.reconsume(State::Data)
            }
        }
    }

    pub(super) fn end_tag_open<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_alpha(c) => {
                self.new_tag(true);
                self.reconsume(State::TagName)
            }
            0x3E => {
                sink.error(ParseError::MissingEndTagName);
                self.switch(State::Data)
            }
            EOF => {
                sink.error(ParseError::EofBeforeTagName);
                Self::emit_bytes(sink, b"</");
                self.emit_eof(sink)
            }
            _ => {
                sink.error(ParseError::InvalidFirstCharacterOfTagName);
                self.comment.clear();
                self.reconsume(State::BogusComment)
            }
        }
    }

    pub(super) fn tag_name<S: Sink>(&mut self, sink: &mut S) -> Go {
        let (a, b) = self.input.run(&NAME_STOPS);
        self.tag.name.extend_from_slice(self.input.bytes(a, b));
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => self.switch(State::BeforeAttributeName),
            0x2F => self.switch(State::SelfClosingStartTag),
            0x3E => {
                self.state = State::Data;
                self.emit_tag(sink)
            }
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                push_char(&mut self.tag.name, REPLACEMENT);
                Go::On
            }
            EOF => {
                sink.error(ParseError::EofInTag);
                self.emit_eof(sink)
            }
            _ => {
                push_char(&mut self.tag.name, lower(c));
                Go::On
            }
        }
    }

    pub(super) fn before_attribute_name<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => Go::On,
            0x2F | 0x3E | EOF => self.reconsume(State::AfterAttributeName),
            0x3D => {
                sink.error(ParseError::UnexpectedEqualsSignBeforeAttributeName);
                self.new_attribute();
                self.tag
                    .attrs
                    .last_mut()
                    .expect("just added")
                    .name
                    .push(b'=');
                self.switch(State::AttributeName)
            }
            _ => {
                self.new_attribute();
                self.reconsume(State::AttributeName)
            }
        }
    }

    pub(super) fn attribute_name<S: Sink>(&mut self, sink: &mut S) -> Go {
        let (a, b) = self.input.run(&NAME_STOPS);
        if b > a {
            let bytes = self.input.bytes(a, b);
            self.tag
                .attrs
                .last_mut()
                .expect("an attribute is being read")
                .name
                .extend_from_slice(bytes);
        }
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) || c == 0x2F || c == 0x3E || c == EOF => {
                self.attribute_name_done(sink);
                self.reconsume(State::AfterAttributeName)
            }
            0x3D => {
                self.attribute_name_done(sink);
                self.switch(State::BeforeAttributeValue)
            }
            _ => {
                let c = match c {
                    NUL => {
                        sink.error(ParseError::UnexpectedNullCharacter);
                        REPLACEMENT
                    }
                    0x22 | 0x27 | 0x3C => {
                        sink.error(ParseError::UnexpectedCharacterInAttributeName);
                        c
                    }
                    _ => lower(c),
                };
                push_char(
                    &mut self
                        .tag
                        .attrs
                        .last_mut()
                        .expect("an attribute is being read")
                        .name,
                    c,
                );
                Go::On
            }
        }
    }

    pub(super) fn after_attribute_name<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => Go::On,
            0x2F => self.switch(State::SelfClosingStartTag),
            0x3D => self.switch(State::BeforeAttributeValue),
            0x3E => {
                self.state = State::Data;
                self.emit_tag(sink)
            }
            EOF => {
                sink.error(ParseError::EofInTag);
                self.emit_eof(sink)
            }
            _ => {
                self.new_attribute();
                self.reconsume(State::AttributeName)
            }
        }
    }

    pub(super) fn before_attribute_value<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => Go::On,
            0x22 => self.switch(State::AttributeValueDoubleQuoted),
            0x27 => self.switch(State::AttributeValueSingleQuoted),
            0x3E => {
                sink.error(ParseError::MissingAttributeValue);
                self.state = State::Data;
                self.emit_tag(sink)
            }
            _ => self.reconsume(State::AttributeValueUnquoted),
        }
    }

    pub(super) fn attribute_value_quoted<S: Sink>(&mut self, sink: &mut S, quote: u8) -> Go {
        let (stops, here) = if quote == b'"' {
            (&DOUBLE_QUOTED_STOPS, State::AttributeValueDoubleQuoted)
        } else {
            (&SINGLE_QUOTED_STOPS, State::AttributeValueSingleQuoted)
        };
        self.value_run(stops);
        let c = getc!(self, sink);
        match c {
            _ if c == u32::from(quote) => self.switch(State::AfterAttributeValueQuoted),
            0x26 => {
                self.return_state = here;
                self.switch(State::CharacterReference)
            }
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                push_char(self.attr_value(), REPLACEMENT);
                Go::On
            }
            EOF => {
                sink.error(ParseError::EofInTag);
                self.emit_eof(sink)
            }
            _ => {
                push_char(self.attr_value(), c);
                Go::On
            }
        }
    }

    fn value_run(&mut self, stops: &[bool; 128]) {
        let (a, b) = self.input.run(stops);
        if b > a {
            let bytes = self.input.bytes(a, b);
            self.tag
                .attrs
                .last_mut()
                .expect("an attribute is being read")
                .value
                .extend_from_slice(bytes);
        }
    }

    pub(super) fn attribute_value_unquoted<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.value_run(&UNQUOTED_STOPS);
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => self.switch(State::BeforeAttributeName),
            0x26 => {
                self.return_state = State::AttributeValueUnquoted;
                self.switch(State::CharacterReference)
            }
            0x3E => {
                self.state = State::Data;
                self.emit_tag(sink)
            }
            NUL => {
                sink.error(ParseError::UnexpectedNullCharacter);
                push_char(self.attr_value(), REPLACEMENT);
                Go::On
            }
            0x22 | 0x27 | 0x3C | 0x3D | 0x60 => {
                sink.error(ParseError::UnexpectedCharacterInUnquotedAttributeValue);
                push_char(self.attr_value(), c);
                Go::On
            }
            EOF => {
                sink.error(ParseError::EofInTag);
                self.emit_eof(sink)
            }
            _ => {
                push_char(self.attr_value(), c);
                Go::On
            }
        }
    }

    pub(super) fn after_attribute_value_quoted<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            _ if is_space(c) => self.switch(State::BeforeAttributeName),
            0x2F => self.switch(State::SelfClosingStartTag),
            0x3E => {
                self.state = State::Data;
                self.emit_tag(sink)
            }
            EOF => {
                sink.error(ParseError::EofInTag);
                self.emit_eof(sink)
            }
            _ => {
                sink.error(ParseError::MissingWhitespaceBetweenAttributes);
                self.reconsume(State::BeforeAttributeName)
            }
        }
    }

    pub(super) fn self_closing_start_tag<S: Sink>(&mut self, sink: &mut S) -> Go {
        let c = getc!(self, sink);
        match c {
            0x3E => {
                self.tag.self_closing = true;
                self.state = State::Data;
                self.emit_tag(sink)
            }
            EOF => {
                sink.error(ParseError::EofInTag);
                self.emit_eof(sink)
            }
            _ => {
                sink.error(ParseError::UnexpectedSolidusInTag);
                self.reconsume(State::BeforeAttributeName)
            }
        }
    }
}
