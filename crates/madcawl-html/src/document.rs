//! A document parsed from the bytes it arrives as: the encoding
//! sniffing algorithm (13.2.3.2) in front of the tokenizer and the tree
//! builder, and the parse started over when a `meta` changes a
//! tentative encoding (13.2.3.4).

use std::cell::Cell;

use madcawl_str::{Atom, atom};

use crate::encoding::{Confidence, Decoder, Encoding, bom, extract_from_content, is_utf16, prescan};
use crate::error::ParseError;
use crate::token::Token;
use crate::tokenizer::{Flow, Sink, Step, Tokenizer};
use crate::tree::{Attrs, Namespace, Options, QuirksMode, TreeBuilder, TreeSink};

/// While the encoding is tentative the bytes are kept to decode again;
/// past this many the tentative encoding is taken as certain.
const TENTATIVE_MAX: usize = 512 * 1024;

/// The prescan reads no further than this.
const PRESCAN: usize = 1024;

/// What is known about a document's encoding before its bytes.
#[derive(Clone, Copy, Debug)]
pub struct Source {
    /// The charset the transport layer gave, as a `Content-Type`
    /// header's; it wins over everything but a byte order mark.
    pub transport: Option<Encoding>,
    /// Used when nothing names an encoding.
    pub fallback: Encoding,
}

impl Default for Source {
    fn default() -> Self {
        Source {
            transport: None,
            fallback: Encoding::Windows1252,
        }
    }
}

/// Parses a document from bytes in any encoding. `make` gives a new,
/// empty sink: once at the start, and again each time a `meta` makes
/// the parse start over.
pub struct DocumentParser<D: TreeSink, F: FnMut() -> D> {
    make: F,
    opts: Options,
    source: Source,
    /// Bytes that may have to be decoded again: all of them until the
    /// encoding is chosen, then all of them while it is tentative.
    held: Vec<u8>,
    sniffed: bool,
    ended: bool,
    encoding: Encoding,
    confidence: Confidence,
    decoder: Decoder,
    text: Vec<u8>,
    tok: Tokenizer,
    builder: TreeBuilder<Watch<D>>,
}

impl<D: TreeSink, F: FnMut() -> D> DocumentParser<D, F> {
    pub fn new(mut make: F, opts: Options, source: Source) -> Self {
        let sink = make();
        DocumentParser {
            make,
            opts,
            source,
            held: Vec::new(),
            sniffed: false,
            ended: false,
            encoding: source.fallback,
            confidence: Confidence::Tentative,
            decoder: Decoder::new(source.fallback),
            text: Vec::new(),
            tok: Tokenizer::new(),
            builder: TreeBuilder::new(Watch::new(sink), opts),
        }
    }

    /// More bytes of the document.
    pub fn feed(&mut self, bytes: &[u8]) {
        if !self.sniffed {
            self.held.extend_from_slice(bytes);
            self.sniff();
            return;
        }
        if self.confidence == Confidence::Tentative {
            self.held.extend_from_slice(bytes);
            if self.held.len() > TENTATIVE_MAX {
                // a meta this late no longer starts the parse over, so the
                // copy kept for that stays bounded
                self.builder.sink().tentative.set(None);
                self.certain();
            }
        }
        self.decode(bytes, false);
    }

    /// No more bytes will come.
    pub fn end(&mut self) {
        self.ended = true;
        if !self.sniffed {
            self.sniff();
        }
        self.decode(&[], true);
        self.tok.end();
    }

    /// Parse what has arrived, as `Tokenizer::run` does. A `meta` that
    /// changes the encoding starts the parse over inside this call.
    pub fn run(&mut self) -> Step {
        loop {
            let step = self.tok.run(&mut Pausing(&mut self.builder));
            let watch = self.builder.sink();
            if let Some(e) = watch.change {
                self.restart(e);
                continue;
            }
            if self.sniffed
                && self.confidence == Confidence::Tentative
                && watch.tentative.get().is_none()
            {
                self.certain();
            }
            if step == Step::Done {
                self.held = Vec::new();
            }
            return step;
        }
    }

    /// The encoding the document is being decoded with.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn confidence(&self) -> Confidence {
        self.confidence
    }

    pub fn sink(&self) -> &D {
        &self.builder.sink().inner
    }

    pub fn into_sink(self) -> D {
        self.builder.into_sink().inner
    }

    /// Choose the encoding once enough bytes have arrived to.
    fn sniff(&mut self) {
        let Some(mark) = bom(&self.held, self.ended) else {
            return;
        };
        let (encoding, confidence, skip) = match (mark, self.source.transport) {
            (Some((e, n)), _) => (e, Confidence::Certain, n),
            (None, Some(e)) => (e, Confidence::Certain, 0),
            (None, None) => {
                if self.held.len() < PRESCAN && !self.ended {
                    return;
                }
                let head = &self.held[..self.held.len().min(PRESCAN)];
                let e = prescan(head).unwrap_or(self.source.fallback);
                (e, Confidence::Tentative, 0)
            }
        };
        self.sniffed = true;
        self.encoding = encoding;
        self.decoder = Decoder::new(encoding);
        let held = std::mem::take(&mut self.held);
        self.decode(&held[skip..], false);
        if confidence == Confidence::Tentative {
            self.builder.sink().tentative.set(Some(encoding));
            self.held = held;
        } else {
            self.confidence = Confidence::Certain;
        }
    }

    fn certain(&mut self) {
        self.confidence = Confidence::Certain;
        self.held = Vec::new();
    }

    /// Throw away what was built and parse every byte again in
    /// `encoding`, which is now certain.
    fn restart(&mut self, encoding: Encoding) {
        self.encoding = encoding;
        self.decoder = Decoder::new(encoding);
        self.tok = Tokenizer::new();
        self.builder = TreeBuilder::new(Watch::new((self.make)()), self.opts);
        let held = std::mem::take(&mut self.held);
        self.certain();
        self.decode(&held, self.ended);
        if self.ended {
            self.tok.end();
        }
    }

    fn decode(&mut self, bytes: &[u8], last: bool) {
        self.text.clear();
        self.decoder.decode(bytes, &mut self.text, last);
        self.tok.feed(&self.text);
    }
}

/// The document's sink, watching for the `meta` elements the "in head"
/// insertion mode creates, the only place an HTML `meta` is created.
struct Watch<D> {
    inner: D,
    /// The encoding in use while it is tentative
    tentative: Cell<Option<Encoding>>,
    /// The encoding a `meta` changed to: the parse starts over in it
    change: Option<Encoding>,
}

impl<D> Watch<D> {
    fn new(inner: D) -> Self {
        Watch {
            inner,
            tentative: Cell::new(None),
            change: None,
        }
    }

    fn meta(&mut self, attrs: Attrs<'_>) {
        let Some(current) = self.tentative.get() else {
            return;
        };
        let value = |name: Atom| attrs.iter().find(|a| a.local == name).map(|a| a.value);
        let pragma = || {
            let equiv = value(atom!("http-equiv"))?;
            if !equiv.eq_ignore_ascii_case(b"content-type") {
                return None;
            }
            extract_from_content(value(atom!("content"))?)
        };
        let named = value(atom!("charset"))
            .and_then(Encoding::for_label)
            .or_else(pragma);
        let Some(new) = named else {
            return;
        };
        // "change the encoding"
        self.tentative.set(None);
        if is_utf16(current) {
            return;
        }
        let new = match new {
            Encoding::Utf16be | Encoding::Utf16le => Encoding::Utf8,
            Encoding::XUserDefined => Encoding::Windows1252,
            e => e,
        };
        if new != current {
            self.change = Some(new);
        }
    }
}

impl<D: TreeSink> TreeSink for Watch<D> {
    type Handle = D::Handle;

    fn document(&self) -> Self::Handle {
        self.inner.document()
    }
    fn create_element(&mut self, ns: Namespace, local: Atom, attrs: Attrs<'_>) -> Self::Handle {
        if ns == Namespace::Html && local == atom!("meta") {
            self.meta(attrs);
        }
        self.inner.create_element(ns, local, attrs)
    }
    fn create_comment(&mut self, data: &[u8]) -> Self::Handle {
        self.inner.create_comment(data)
    }
    fn create_processing_instruction(&mut self, target: &[u8], data: &[u8]) -> Self::Handle {
        self.inner.create_processing_instruction(target, data)
    }
    fn append_doctype(&mut self, name: &[u8], public_id: &[u8], system_id: &[u8]) {
        self.inner.append_doctype(name, public_id, system_id)
    }
    fn append(&mut self, parent: Self::Handle, child: Self::Handle) {
        self.inner.append(parent, child)
    }
    fn insert_before(&mut self, parent: Self::Handle, child: Self::Handle, before: Self::Handle) {
        self.inner.insert_before(parent, child, before)
    }
    fn insert_text(&mut self, parent: Self::Handle, before: Option<Self::Handle>, text: &[u8]) {
        self.inner.insert_text(parent, before, text)
    }
    fn remove(&mut self, node: Self::Handle) {
        self.inner.remove(node)
    }
    fn reparent_children(&mut self, from: Self::Handle, to: Self::Handle) {
        self.inner.reparent_children(from, to)
    }
    fn parent(&self, node: Self::Handle) -> Option<Self::Handle> {
        self.inner.parent(node)
    }
    fn add_attrs_if_missing(&mut self, element: Self::Handle, attrs: Attrs<'_>) {
        self.inner.add_attrs_if_missing(element, attrs)
    }
    fn template_contents(&mut self, template: Self::Handle) -> Self::Handle {
        self.inner.template_contents(template)
    }
    fn set_quirks_mode(&mut self, mode: QuirksMode) {
        self.inner.set_quirks_mode(mode)
    }
    fn associate_form(&mut self, element: Self::Handle, form: Self::Handle, parent: Self::Handle) {
        self.inner.associate_form(element, form, parent)
    }
    fn children_finished(&mut self, element: Self::Handle) {
        self.inner.children_finished(element)
    }
    fn parse_error(&mut self, error: ParseError) {
        self.inner.parse_error(error)
    }
    fn is_full(&self) -> bool {
        self.inner.is_full()
    }
}

/// The tree builder as the tokenizer's sink, pausing the tokenizer as
/// soon as a `meta` has changed the encoding.
struct Pausing<'a, D: TreeSink>(&'a mut TreeBuilder<Watch<D>>);

impl<D: TreeSink> Sink for Pausing<'_, D> {
    fn token(&mut self, token: Token<'_>) -> Flow {
        let flow = self.0.token(token);
        if self.0.sink().change.is_some() {
            Flow::Pause
        } else {
            flow
        }
    }
    fn error(&mut self, error: ParseError) {
        self.0.error(error)
    }
    fn in_foreign_content(&self) -> bool {
        self.0.in_foreign_content()
    }
}
