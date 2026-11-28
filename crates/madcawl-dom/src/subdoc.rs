//! Documents of child navigables: active documents besides the DOM's
//! own, which the bindings make for iframes, parse into, and empty when
//! the iframe goes.

use madcawl_html::{Attrs, Namespace, Options, ParseError, QuirksMode, Step, Tokenizer, TreeBuilder, TreeSink};
use madcawl_str::{Atom, atom};

use crate::dom::{Dom, NodeId};
use crate::held::HeldNode;

/// The DOM as a document parser's sink for another of its documents:
/// the DOM's own sink, with that document standing in for its own.
struct Other<'a> {
    dom: &'a mut Dom,
}

impl TreeSink for Other<'_> {
    type Handle = NodeId;

    fn document(&self) -> NodeId {
        TreeSink::document(&*self.dom)
    }
    fn create_element(&mut self, ns: Namespace, local: Atom, attrs: Attrs<'_>) -> NodeId {
        self.dom.create_element(ns, local, attrs)
    }
    fn create_comment(&mut self, data: &[u8]) -> NodeId {
        self.dom.create_comment(data)
    }
    fn create_processing_instruction(&mut self, target: &[u8], data: &[u8]) -> NodeId {
        self.dom.create_processing_instruction(target, data)
    }
    fn append_doctype(&mut self, name: &[u8], public_id: &[u8], system_id: &[u8]) {
        self.dom.append_doctype(name, public_id, system_id)
    }
    fn append(&mut self, parent: NodeId, child: NodeId) {
        self.dom.append(parent, child)
    }
    fn insert_before(&mut self, parent: NodeId, child: NodeId, before: NodeId) {
        self.dom.insert_before(parent, child, before)
    }
    fn insert_text(&mut self, parent: NodeId, before: Option<NodeId>, text: &[u8]) {
        self.dom.insert_text(parent, before, text)
    }
    fn remove(&mut self, node: NodeId) {
        TreeSink::remove(self.dom, node)
    }
    fn reparent_children(&mut self, from: NodeId, to: NodeId) {
        self.dom.reparent_children(from, to)
    }
    fn parent(&self, node: NodeId) -> Option<NodeId> {
        Dom::parent(self.dom, node)
    }
    fn add_attrs_if_missing(&mut self, element: NodeId, attrs: Attrs<'_>) {
        self.dom.add_attrs_if_missing(element, attrs)
    }
    fn template_contents(&mut self, template: NodeId) -> NodeId {
        self.dom.template_contents(template)
    }
    fn set_quirks_mode(&mut self, mode: QuirksMode) {
        self.dom.set_quirks_mode(mode)
    }
    fn associate_form(&mut self, element: NodeId, form: NodeId, intended_parent: NodeId) {
        self.dom.associate_form(element, form, intended_parent)
    }
    fn children_finished(&mut self, element: NodeId) {
        self.dom.children_finished(element)
    }
    fn parse_error(&mut self, error: ParseError) {
        self.dom.parse_error(error)
    }
    fn is_full(&self) -> bool {
        TreeSink::is_full(&*self.dom)
    }
}

impl Dom {
    /// An active document besides this DOM's own, held by the handle
    /// returned; with `blank`, holding the tree of `about:blank` (`html`,
    /// `head`, `body`), otherwise empty. `None` when the DOM holds all
    /// the nodes it may.
    pub fn new_active_document(&mut self, blank: bool) -> Option<HeldNode> {
        let room = if blank { 4 } else { 1 };
        if self.arena.count() + room > self.limits.nodes {
            self.cut = Some(crate::Refused::Nodes);
            return None;
        }
        let doc = self.new_document(false).expect("room checked");
        let held = self.hold(NodeId::at(doc));
        if blank {
            let html = self.element(Namespace::Html, atom!("html"), doc).expect("room checked");
            self.insert(doc, html, 0);
            for name in [atom!("head"), atom!("body")] {
                let el = self.element(Namespace::Html, name, doc).expect("room checked");
                self.insert(html, el, 0);
            }
        }
        Some(held)
    }

    /// Parse `html` (WTF-8) into `doc`, an empty active document of
    /// this DOM, as a document's parser with scripting enabled does, all
    /// at once: the script elements are left as the parser made them,
    /// for the caller to mark. Whether everything parsed fit under the
    /// DOM's limits.
    pub fn parse_into(&mut self, doc: NodeId, html: &[u8]) -> bool {
        assert!(self.first_child(doc).is_none() && !self.is_inert_document(doc), "an empty active document");
        // the refusals so far were script's: they do not stop this parser
        let (main, cut, stopped) = (self.doc, self.cut.take(), std::mem::take(&mut self.stopped));
        self.doc = doc.index();
        let mut b = TreeBuilder::new(Other { dom: self }, Options { scripting: true, ..Options::default() });
        let mut tok = Tokenizer::new();
        tok.feed(html);
        tok.end();
        assert_eq!(tok.run(&mut b), Step::Done, "a parser that pauses for no script");
        drop(b);
        self.doc = main;
        let complete = !self.stopped && self.cut.is_none();
        (self.cut, self.stopped) = (cut, stopped);
        complete
    }

    /// Take every child out of document `doc`, whose navigable is gone:
    /// what was below it goes by count, or to the cycle collector, as
    /// any removed tree does.
    pub fn empty_document(&mut self, doc: NodeId) {
        assert!(doc != self.document(), "a document besides the DOM's own");
        while let Some(c) = self.first_child(doc) {
            self.detach(c.index());
        }
    }

    /// What `make` makes, with `doc` as the node document in place of
    /// this DOM's own: `|d| d.new_element(b"p")` makes a `p` of `doc`.
    pub fn in_document<T>(&mut self, doc: NodeId, make: impl FnOnce(&mut Dom) -> T) -> T {
        assert_eq!(self.kind(doc), crate::NodeKind::Document, "a document");
        let main = std::mem::replace(&mut self.doc, doc.index());
        let made = make(self);
        self.doc = main;
        made
    }

    /// From now on (`on`) record the HTML `iframe` elements the parser
    /// finishes, for [`Dom::take_parsed_frames`]; or stop, dropping what
    /// is recorded.
    pub fn record_parsed_frames(&mut self, on: bool) {
        self.parsed_frames = on.then(Vec::new);
    }

    /// The iframes recorded since the last call, in the order the parser
    /// finished them.
    pub fn take_parsed_frames(&mut self, out: &mut Vec<NodeId>) {
        if let Some(r) = self.parsed_frames.as_mut() {
            out.extend(r.drain(..).map(NodeId::at));
        }
    }

    pub(crate) fn frame_finished(&mut self, i: u32) {
        let iframe = self.parsed_frames.is_some() && self.is_html(i, atom!("iframe"));
        if let Some(r) = self.parsed_frames.as_mut().filter(|_| iframe) {
            r.push(i);
        }
    }
}
