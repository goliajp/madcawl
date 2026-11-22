//! The tree construction stage: tokens in, calls on a `TreeSink` out.

mod adoption;
mod body;
mod body_end;
mod foreign;
mod formatting;
mod head;
mod insert;
mod names;
mod other;
mod quirks;
mod sink;
mod stack;
mod table;
mod tag;
mod types;

use madcawl_str::{Atom, HeldAtoms};

use crate::error::ParseError;
use crate::token::{Attribute, Token};
use crate::tokenizer::{Flow, Sink, State};
pub use sink::{AttrNamespace, AttrRef, Attrs, Namespace, QuirksMode, TreeSink};
use tag::{Name, Tag};
pub(crate) use types::{Active, Class, Mode, Open, Start, Step, Tok};

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Scripting is enabled: `<noscript>` content is raw text.
    pub scripting: bool,
    /// The document is an iframe `srcdoc` document, never in quirks mode.
    pub iframe_srcdoc: bool,
}

/// The element a fragment is parsed for, as `innerHTML` does.
pub struct FragmentContext<'a, H> {
    pub ns: Namespace,
    pub local: Atom,
    pub attrs: &'a [Attribute],
    /// The nearest form element at or above the context element.
    pub form: Option<H>,
    pub quirks: QuirksMode,
}

pub struct TreeBuilder<S: TreeSink> {
    sink: S,
    doc: S::Handle,
    opts: Options,
    mode: Mode,
    original_mode: Mode,
    template_modes: Vec<Mode>,
    open: stack::OpenStack<S::Handle>,
    active: Vec<Active<S::Handle>>,
    /// Attribute lists of formatting entries that are gone, kept for
    /// their allocations.
    spare_attrs: Vec<Vec<Attribute>>,
    head: Option<S::Handle>,
    form: Option<S::Handle>,
    /// The fragment context element; its node is the root, never used.
    context: Option<Open<S::Handle>>,
    /// The names that are not static atoms, held for the whole parse so
    /// that a name on the stack stays the same atom.
    names: HeldAtoms,
    frameset_ok: bool,
    foster: bool,
    quirks: QuirksMode,
    skip_lf: bool,
    text: Vec<u8>,
    text_at: Option<(S::Handle, Option<S::Handle>)>,
    table_text: Vec<u8>,
    table_text_dirty: bool,
    next_state: Option<State>,
    stopped: bool,
    /// The depth limit has been hit once; it is reported once.
    depth_capped: bool,
}

impl<S: TreeSink> TreeBuilder<S> {
    pub fn new(sink: S, opts: Options) -> Self {
        let doc = sink.document();
        Self {
            sink,
            doc,
            opts,
            mode: Mode::Initial,
            original_mode: Mode::Initial,
            template_modes: Vec::new(),
            open: stack::OpenStack::new(),
            active: Vec::new(),
            spare_attrs: Vec::new(),
            head: None,
            form: None,
            context: None,
            names: HeldAtoms::new(),
            frameset_ok: true,
            foster: false,
            quirks: QuirksMode::NoQuirks,
            skip_lf: false,
            text: Vec::new(),
            text_at: None,
            table_text: Vec::new(),
            table_text_dirty: false,
            next_state: None,
            stopped: false,
            depth_capped: false,
        }
    }

    /// A builder for a fragment parsed in `ctx`. Start the tokenizer in
    /// `tokenizer_state`.
    pub fn new_fragment(sink: S, opts: Options, ctx: FragmentContext<'_, S::Handle>) -> Self {
        let mut b = Self::new(sink, opts);
        b.quirks = ctx.quirks;
        b.sink.set_quirks_mode(ctx.quirks);
        let root = b
            .sink
            .create_element(Namespace::Html, Tag::Html.atom(), Attrs::empty());
        b.sink.append(b.doc, root);
        b.open.push(Open {
            node: root,
            ns: Namespace::Html,
            name: Name::known(Tag::Html),
            html_ip: false,
        });
        let name = Name::lowercase(ctx.local, &b.names);
        let start = Start {
            name,
            attrs: ctx.attrs,
            self_closing: false,
        };
        b.context = Some(Open {
            node: root,
            ns: ctx.ns,
            name,
            html_ip: insert::is_html_ip(ctx.ns, &start),
        });
        if ctx.ns == Namespace::Html && name.tag == Tag::Template {
            b.template_modes.push(Mode::InTemplate);
        }
        b.reset_mode();
        b.form = ctx.form;
        b
    }

    /// The tokenizer state the fragment context selects.
    pub fn tokenizer_state(&self) -> State {
        let Some(ctx) = self.context.filter(|c| c.ns == Namespace::Html) else {
            return State::Data;
        };
        match ctx.name.tag {
            Tag::Title | Tag::Textarea => State::Rcdata,
            Tag::Style | Tag::Xmp | Tag::Iframe | Tag::Noembed | Tag::Noframes => State::Rawtext,
            Tag::Noscript if self.opts.scripting => State::Rawtext,
            Tag::Script => State::ScriptData,
            Tag::Plaintext => State::Plaintext,
            _ => State::Data,
        }
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    pub fn into_sink(self) -> S {
        self.sink
    }

    fn adjusted_current(&self) -> Option<&Open<S::Handle>> {
        match &self.context {
            Some(c) if self.open.len() == 1 => Some(c),
            _ => self.open.last(),
        }
    }

    fn html_rules(&self, tok: &Tok<'_>) -> bool {
        let Some(n) = self.adjusted_current() else {
            return true;
        };
        if n.ns == Namespace::Html {
            return true;
        }
        match tok {
            Tok::Start(s) => {
                (n.mathml_text_ip() && !matches!(s.name.tag, Tag::Mglyph | Tag::Malignmark))
                    || (n.is(Namespace::MathMl, Tag::AnnotationXml) && s.name.tag == Tag::Svg)
                    || n.html_ip
            }
            Tok::Text(..) => n.mathml_text_ip() || n.html_ip,
            Tok::Eof => true,
            _ => false,
        }
    }

    fn process(&mut self, tok: Tok<'_>) {
        let mut html = false;
        loop {
            let step = if html || self.html_rules(&tok) {
                self.by_mode(self.mode, tok)
            } else {
                self.foreign(tok)
            };
            match step {
                Step::Done => return,
                Step::Again => html = false,
                Step::AgainHtml => html = true,
            }
        }
    }

    fn by_mode(&mut self, mode: Mode, tok: Tok<'_>) -> Step {
        match mode {
            Mode::Initial => self.initial(tok),
            Mode::BeforeHtml => self.before_html(tok),
            Mode::BeforeHead => self.before_head(tok),
            Mode::InHead => self.in_head(tok),
            Mode::InHeadNoscript => self.in_head_noscript(tok),
            Mode::AfterHead => self.after_head(tok),
            Mode::InBody => self.in_body(tok),
            Mode::Text => self.text_mode(tok),
            Mode::InTable => self.in_table(tok),
            Mode::InTableText => self.in_table_text(tok),
            Mode::InCaption => self.in_caption(tok),
            Mode::InColumnGroup => self.in_column_group(tok),
            Mode::InTableBody => self.in_table_body(tok),
            Mode::InRow => self.in_row(tok),
            Mode::InCell => self.in_cell(tok),
            Mode::InTemplate => self.in_template(tok),
            Mode::AfterBody => self.after_body(tok),
            Mode::InFrameset => self.in_frameset(tok),
            Mode::AfterFrameset => self.after_frameset(tok),
            Mode::AfterAfterBody => self.after_after_body(tok),
            Mode::AfterAfterFrameset => self.after_after_frameset(tok),
        }
    }

    fn chars(&mut self, mut s: &[u8]) {
        if self.skip_lf {
            self.skip_lf = false;
            if s.first() == Some(&b'\n') {
                s = &s[1..];
            }
        }
        while let Some(&first) = s.first() {
            let class = class_of(first);
            let n = s
                .iter()
                .position(|&b| class_of(b) != class)
                .unwrap_or(s.len());
            self.process(Tok::Text(class, &s[..n]));
            s = &s[n..];
        }
    }

    /// Stop parsing: every open element is done.
    fn stop(&mut self) {
        self.flush_text();
        while !self.open.is_empty() {
            self.pop();
        }
        self.stopped = true;
    }
}

fn class_of(b: u8) -> Class {
    match b {
        b'\t' | b'\n' | 0x0C | b'\r' | b' ' => Class::Space,
        0 => Class::Nul,
        _ => Class::Other,
    }
}

impl<S: TreeSink> Sink for TreeBuilder<S> {
    fn token(&mut self, token: Token<'_>) -> Flow {
        if self.stopped {
            return Flow::Continue;
        }
        if self.sink.is_full() {
            self.sink.parse_error(ParseError::DocumentTooLarge);
            self.stop();
            return Flow::Continue;
        }
        if let Token::Chars(s) = token {
            self.chars(s);
        } else {
            self.flush_text();
            self.skip_lf = false;
            let tok = match token {
                Token::StartTag(t) => Tok::Start(Start {
                    name: Name::of_tag(t, &self.names),
                    attrs: &t.attrs,
                    self_closing: t.self_closing,
                }),
                Token::EndTag(t) => Tok::End(Name::of_tag(t, &self.names)),
                Token::Comment(s) => Tok::Comment(s),
                Token::ProcessingInstruction { target, data } => Tok::Pi(target, data),
                Token::Doctype(d) => Tok::Doctype(d),
                Token::Eof => Tok::Eof,
                Token::Chars(_) => unreachable!(),
            };
            self.process(tok);
            if matches!(tok, Tok::Eof) && !self.stopped {
                self.stop();
            }
        }
        match self.next_state.take() {
            Some(state) => Flow::Switch(state),
            None => Flow::Continue,
        }
    }

    fn error(&mut self, error: ParseError) {
        self.sink.parse_error(error);
    }

    fn in_foreign_content(&self) -> bool {
        self.adjusted_current()
            .is_some_and(|n| n.ns != Namespace::Html)
    }
}
