//! The tokenizer of the HTML standard (13.2.5), as a state machine that
//! can stop wherever the input runs out and go on when more arrives.

mod charref;
mod comment;
mod doctype;
mod pi;
mod script;
mod state;
mod tag;
mod text;

use crate::error::ParseError;
use crate::input::{Input, encode_char};
use crate::token::{Attribute, Doctype, Tag, Token};

/// Up to this many attributes on a tag, a new one is compared with each
/// of the others; past it, they go in a hash set.
const LINEAR_ATTRS: usize = 8;
pub use state::State;

/// What the sink wants after a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// Go on in this state: the tree builder does this after `<title>`,
    /// `<script>` and the like.
    Switch(State),
    /// Stop after this token, as before running a script; `run` again
    /// to go on.
    Pause,
}

/// Why `run` returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Everything that has arrived is read; `feed` more, or `end`.
    NeedInput,
    /// The sink asked to pause.
    Paused,
    /// The end-of-file token has been emitted.
    Done,
}

/// Where tokens go: the tree builder, or a test.
pub trait Sink {
    fn token(&mut self, token: Token<'_>) -> Flow;
    fn error(&mut self, _error: ParseError) {}
    /// Whether the adjusted current node is in a namespace other than
    /// HTML, where `<![CDATA[` opens a CDATA section.
    fn in_foreign_content(&self) -> bool {
        false
    }
}

/// What a state handler tells the loop.
pub(crate) enum Go {
    On,
    Stop(Step),
}

/// Read the next code point, or return from the state handler when
/// the rest has not arrived.
macro_rules! getc {
    ($t:ident, $sink:ident) => {{
        let c = $t.input.next();
        if c == crate::input::MORE {
            return crate::tokenizer::Go::Stop(crate::tokenizer::Step::NeedInput);
        }
        if let Some(e) = $t.input.error.take() {
            $sink.error(e);
        }
        c
    }};
}
pub(crate) use getc;

pub struct Tokenizer {
    input: Input,
    state: State,
    return_state: State,
    tag: Tag,
    is_end_tag: bool,
    /// Attributes of earlier tags, kept for their allocations.
    spare_attrs: Vec<Attribute>,
    /// The attribute being read repeats an earlier one and is dropped
    /// when it ends.
    attr_is_duplicate: bool,
    /// The names of the current tag's attributes once it has more than
    /// `LINEAR_ATTRS`, so a tag with many of them is checked for a
    /// repeat in constant time each rather than against all before it.
    attr_names: std::collections::HashSet<Vec<u8>>,
    last_start_tag: Vec<u8>,
    temp: Vec<u8>,
    comment: Vec<u8>,
    pi_data: Vec<u8>,
    doctype: Doctype,
    char_ref: u32,
    done: bool,
}

impl Default for Tokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Tokenizer {
    pub fn new() -> Self {
        Self {
            input: Input::default(),
            state: State::Data,
            return_state: State::Data,
            tag: Tag::default(),
            is_end_tag: false,
            spare_attrs: Vec::new(),
            attr_is_duplicate: false,
            attr_names: std::collections::HashSet::new(),
            last_start_tag: Vec::new(),
            temp: Vec::new(),
            comment: Vec::new(),
            pi_data: Vec::new(),
            doctype: Doctype::default(),
            char_ref: 0,
            done: false,
        }
    }

    /// More bytes of the document, WTF-8.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.input.push(bytes);
    }

    /// No more bytes will come.
    pub fn end(&mut self) {
        self.input.finish();
    }

    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    /// The start tag an end tag must match to close RCDATA, RAWTEXT or
    /// script data, as when parsing a fragment.
    pub fn set_last_start_tag(&mut self, name: &[u8]) {
        self.last_start_tag.clear();
        self.last_start_tag.extend_from_slice(name);
    }

    /// Tokenize what has arrived.
    pub fn run<S: Sink>(&mut self, sink: &mut S) -> Step {
        if self.done {
            return Step::Done;
        }
        loop {
            let go = match self.state {
                State::Data => self.data(sink),
                State::Rcdata => self.rcdata(sink),
                State::Rawtext => self.rawtext(sink),
                State::ScriptData => self.script_data(sink),
                State::Plaintext => self.plaintext(sink),
                State::TagOpen => self.tag_open(sink),
                State::EndTagOpen => self.end_tag_open(sink),
                State::TagName => self.tag_name(sink),
                State::RcdataLessThanSign => {
                    self.text_less_than(sink, State::Rcdata, State::RcdataEndTagOpen)
                }
                State::RcdataEndTagOpen => {
                    self.text_end_tag_open(sink, State::Rcdata, State::RcdataEndTagName)
                }
                State::RcdataEndTagName => self.text_end_tag_name(sink, State::Rcdata),
                State::RawtextLessThanSign => {
                    self.text_less_than(sink, State::Rawtext, State::RawtextEndTagOpen)
                }
                State::RawtextEndTagOpen => {
                    self.text_end_tag_open(sink, State::Rawtext, State::RawtextEndTagName)
                }
                State::RawtextEndTagName => self.text_end_tag_name(sink, State::Rawtext),
                State::ScriptDataLessThanSign => self.script_data_less_than(sink),
                State::ScriptDataEndTagOpen => {
                    self.text_end_tag_open(sink, State::ScriptData, State::ScriptDataEndTagName)
                }
                State::ScriptDataEndTagName => self.text_end_tag_name(sink, State::ScriptData),
                State::ScriptDataEscapeStart => self.script_escape_start(sink),
                State::ScriptDataEscapeStartDash => self.script_escape_start_dash(sink),
                State::ScriptDataEscaped => self.script_escaped(sink),
                State::ScriptDataEscapedDash => self.script_escaped_dash(sink),
                State::ScriptDataEscapedDashDash => self.script_escaped_dash_dash(sink),
                State::ScriptDataEscapedLessThanSign => self.script_escaped_less_than(sink),
                State::ScriptDataEscapedEndTagOpen => self.text_end_tag_open(
                    sink,
                    State::ScriptDataEscaped,
                    State::ScriptDataEscapedEndTagName,
                ),
                State::ScriptDataEscapedEndTagName => {
                    self.text_end_tag_name(sink, State::ScriptDataEscaped)
                }
                State::ScriptDataDoubleEscapeStart => self.script_double_escape_edge(sink, true),
                State::ScriptDataDoubleEscaped => self.script_double_escaped(sink),
                State::ScriptDataDoubleEscapedDash => self.script_double_escaped_dash(sink),
                State::ScriptDataDoubleEscapedDashDash => {
                    self.script_double_escaped_dash_dash(sink)
                }
                State::ScriptDataDoubleEscapedLessThanSign => {
                    self.script_double_escaped_less_than(sink)
                }
                State::ScriptDataDoubleEscapeEnd => self.script_double_escape_edge(sink, false),
                State::BeforeAttributeName => self.before_attribute_name(sink),
                State::AttributeName => self.attribute_name(sink),
                State::AfterAttributeName => self.after_attribute_name(sink),
                State::BeforeAttributeValue => self.before_attribute_value(sink),
                State::AttributeValueDoubleQuoted => self.attribute_value_quoted(sink, b'"'),
                State::AttributeValueSingleQuoted => self.attribute_value_quoted(sink, b'\''),
                State::AttributeValueUnquoted => self.attribute_value_unquoted(sink),
                State::AfterAttributeValueQuoted => self.after_attribute_value_quoted(sink),
                State::SelfClosingStartTag => self.self_closing_start_tag(sink),
                State::BogusComment => self.bogus_comment(sink),
                State::MarkupDeclarationOpen => self.markup_declaration_open(sink),
                State::CommentStart => self.comment_start(sink),
                State::CommentStartDash => self.comment_start_dash(sink),
                State::Comment => self.comment(sink),
                State::CommentLessThanSign => self.comment_less_than(sink),
                State::CommentLessThanSignBang => self.comment_less_than_bang(sink),
                State::CommentLessThanSignBangDash => self.comment_less_than_bang_dash(sink),
                State::CommentLessThanSignBangDashDash => {
                    self.comment_less_than_bang_dash_dash(sink)
                }
                State::CommentEndDash => self.comment_end_dash(sink),
                State::CommentEnd => self.comment_end(sink),
                State::CommentEndBang => self.comment_end_bang(sink),
                State::Doctype => self.doctype(sink),
                State::BeforeDoctypeName => self.before_doctype_name(sink),
                State::DoctypeName => self.doctype_name(sink),
                State::AfterDoctypeName => self.after_doctype_name(sink),
                State::AfterDoctypePublicKeyword => self.after_doctype_keyword(sink, true),
                State::BeforeDoctypePublicIdentifier => self.before_doctype_identifier(sink, true),
                State::DoctypePublicIdentifierDoubleQuoted => {
                    self.doctype_identifier(sink, true, b'"')
                }
                State::DoctypePublicIdentifierSingleQuoted => {
                    self.doctype_identifier(sink, true, b'\'')
                }
                State::AfterDoctypePublicIdentifier => self.after_doctype_public_identifier(sink),
                State::BetweenDoctypePublicAndSystemIdentifiers => {
                    self.between_doctype_identifiers(sink)
                }
                State::AfterDoctypeSystemKeyword => self.after_doctype_keyword(sink, false),
                State::BeforeDoctypeSystemIdentifier => self.before_doctype_identifier(sink, false),
                State::DoctypeSystemIdentifierDoubleQuoted => {
                    self.doctype_identifier(sink, false, b'"')
                }
                State::DoctypeSystemIdentifierSingleQuoted => {
                    self.doctype_identifier(sink, false, b'\'')
                }
                State::AfterDoctypeSystemIdentifier => self.after_doctype_system_identifier(sink),
                State::BogusDoctype => self.bogus_doctype(sink),
                State::CdataSection => self.cdata_section(sink),
                State::CdataSectionBracket => self.cdata_section_bracket(sink),
                State::CdataSectionEnd => self.cdata_section_end(sink),
                State::ProcessingInstructionOpen => self.pi_open(sink),
                State::ProcessingInstructionTarget => self.pi_target(sink),
                State::AfterProcessingInstructionTarget => self.after_pi_target(sink),
                State::ProcessingInstructionData => self.pi_data(sink),
                State::ProcessingInstructionQuestionable => self.pi_questionable(sink),
                State::CharacterReference => self.character_reference(sink),
                State::NamedCharacterReference => self.named_character_reference(sink),
                State::AmbiguousAmpersand => self.ambiguous_ampersand(sink),
                State::NumericCharacterReference => self.numeric_character_reference(sink),
                State::HexadecimalCharacterReferenceStart => self.numeric_start(sink, 16),
                State::DecimalCharacterReferenceStart => self.numeric_start(sink, 10),
                State::HexadecimalCharacterReference => self.numeric_digits(sink, 16),
                State::DecimalCharacterReference => self.numeric_digits(sink, 10),
                State::NumericCharacterReferenceEnd => self.numeric_end(sink),
            };
            if let Go::Stop(step) = go {
                return step;
            }
        }
    }

    fn emit_char<S: Sink>(&mut self, sink: &mut S, c: u32) {
        let (b, n) = encode_char(c);
        sink.token(Token::Chars(&b[..n]));
    }

    fn emit_bytes<S: Sink>(sink: &mut S, s: &[u8]) {
        if !s.is_empty() {
            sink.token(Token::Chars(s));
        }
    }

    fn emit_eof<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.done = true;
        sink.token(Token::Eof);
        Go::Stop(Step::Done)
    }

    fn emit_comment<S: Sink>(&mut self, sink: &mut S) {
        sink.token(Token::Comment(&self.comment));
    }

    fn emit_doctype<S: Sink>(&mut self, sink: &mut S) {
        sink.token(Token::Doctype(&self.doctype));
    }

    /// Emit the current tag, already in the data state; the sink may
    /// move the tokenizer elsewhere.
    fn emit_tag<S: Sink>(&mut self, sink: &mut S) -> Go {
        self.end_attribute();
        let flow = if self.is_end_tag {
            if !self.tag.attrs.is_empty() {
                sink.error(ParseError::EndTagWithAttributes);
            }
            if self.tag.self_closing {
                sink.error(ParseError::EndTagWithTrailingSolidus);
            }
            sink.token(Token::EndTag(&self.tag))
        } else {
            self.last_start_tag.clear();
            self.last_start_tag.extend_from_slice(&self.tag.name);
            sink.token(Token::StartTag(&self.tag))
        };
        match flow {
            Flow::Continue => Go::On,
            Flow::Switch(state) => {
                self.state = state;
                Go::On
            }
            Flow::Pause => Go::Stop(Step::Paused),
        }
    }

    fn new_tag(&mut self, end: bool) {
        self.is_end_tag = end;
        self.tag.name.clear();
        self.tag.self_closing = false;
        self.attr_is_duplicate = false;
        self.attr_names.clear();
        self.spare_attrs.append(&mut self.tag.attrs);
    }

    fn new_attribute(&mut self) {
        self.end_attribute();
        let mut a = self.spare_attrs.pop().unwrap_or_default();
        a.name.clear();
        a.value.clear();
        self.tag.attrs.push(a);
    }

    /// The name of the attribute being read is complete.
    fn attribute_name_done<S: Sink>(&mut self, sink: &mut S) {
        let Some((last, earlier)) = self.tag.attrs.split_last() else {
            return;
        };
        let repeat = if earlier.len() < LINEAR_ATTRS {
            step!(earlier.len());
            earlier.iter().any(|a| a.name == last.name)
        } else {
            if self.attr_names.is_empty() {
                self.attr_names
                    .extend(earlier.iter().map(|a| a.name.clone()));
            }
            !self.attr_names.insert(last.name.clone())
        };
        if repeat {
            sink.error(ParseError::DuplicateAttribute);
            self.attr_is_duplicate = true;
        }
    }

    fn end_attribute(&mut self) {
        if self.attr_is_duplicate {
            self.attr_is_duplicate = false;
            if let Some(a) = self.tag.attrs.pop() {
                self.spare_attrs.push(a);
            }
        }
    }

    fn attr_value(&mut self) -> &mut Vec<u8> {
        &mut self
            .tag
            .attrs
            .last_mut()
            .expect("an attribute is being read")
            .value
    }

    /// The end tag being read closes the element whose text this is.
    fn appropriate_end_tag(&self) -> bool {
        !self.last_start_tag.is_empty() && self.tag.name == self.last_start_tag
    }

    fn switch(&mut self, state: State) -> Go {
        self.state = state;
        Go::On
    }

    fn reconsume(&mut self, state: State) -> Go {
        self.input.unread();
        self.state = state;
        Go::On
    }
}

pub(crate) fn is_space(c: u32) -> bool {
    matches!(c, 0x09 | 0x0A | 0x0C | 0x20)
}

pub(crate) fn is_alpha(c: u32) -> bool {
    matches!(c, 0x41..=0x5A | 0x61..=0x7A)
}

pub(crate) fn lower(c: u32) -> u32 {
    if matches!(c, 0x41..=0x5A) {
        c + 0x20
    } else {
        c
    }
}

pub(crate) const NUL: u32 = 0;
pub(crate) const REPLACEMENT: u32 = 0xFFFD;
