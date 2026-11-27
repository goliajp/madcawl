//! The HTML fragment parsing algorithm (HTML 13.4), as setting
//! `innerHTML` and `outerHTML` runs it.
//!
//! The standard parses into a new document and then moves the nodes
//! into a fragment of the context's node document. No script can run
//! during the parse, so the nodes are made in the context's node
//! document from the start; the only node of the new document anyone
//! could tell apart, its `html` root, stays out of every tree and is
//! freed with nothing below it.

use madcawl_html::{
    AttrNamespace, Attribute, Attrs, FragmentContext, Namespace, ParseError, QuirksMode, Step, Tokenizer,
    TreeBuilder, TreeSink,
};
use madcawl_str::{Atom, atom};

use crate::dom::{Dom, HEADROOM, NodeId};
use crate::node::{NodeKind, QUIRKS_MASK, QUIRKS_SHIFT};

/// What [`Dom::parse_fragment`] made: a document fragment, held by
/// nothing yet, and whether everything parsed fit under the DOM's
/// limits.
pub struct Parsed {
    pub fragment: NodeId,
    pub complete: bool,
}

/// The DOM as the fragment parser's sink: the context's node document
/// stands for the new document, which takes no children.
struct Frag<'a> {
    dom: &'a mut Dom,
    doc: NodeId,
    root: Option<NodeId>,
    stopped: bool,
}

impl TreeSink for Frag<'_> {
    type Handle = NodeId;

    fn document(&self) -> NodeId {
        self.doc
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
        // the only node the builder puts into the document is the root
        if parent == self.doc {
            assert!(self.root.is_none(), "one root");
            self.root = Some(child);
        } else {
            self.dom.append(parent, child)
        }
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
    // the new document's mode is the context's, which it already has
    fn set_quirks_mode(&mut self, _: QuirksMode) {}
    fn associate_form(&mut self, element: NodeId, form: NodeId, intended_parent: NodeId) {
        self.dom.associate_form(element, form, intended_parent)
    }
    fn children_finished(&mut self, element: NodeId) {
        self.dom.children_finished(element)
    }
    fn parse_error(&mut self, error: ParseError) {
        self.stopped |= error == ParseError::DocumentTooLarge;
    }
    fn is_full(&self) -> bool {
        self.dom.is_full()
    }
}

impl Dom {
    /// Parse `html` (WTF-8) for element `context` (HTML's fragment
    /// parsing algorithm), the nodes put into a new document fragment of
    /// the context's node document; `None` when the DOM has fewer than
    /// [`HEADROOM`] nodes left, the room a parser starts with.
    /// `scripting` is whether scripting is enabled for the context's
    /// document, which decides how `noscript` content parses. Script
    /// elements in it are left for the caller to mark as already
    /// started. The fragment is held by nothing: the caller holds it for
    /// as long as it uses it.
    pub fn parse_fragment(&mut self, context: NodeId, html: &[u8], scripting: bool) -> Option<Parsed> {
        // the builder makes its root before it first asks for room
        if self.arena.count() + HEADROOM > self.limits.nodes {
            return None;
        }
        let owner = self.n(context.index()).owner;
        let fragment = NodeId::at(self.new_node(NodeKind::Fragment, owner).expect("room for the fragment"));
        let ns = self.namespace(context).expect("an element");
        let local = self.local_name(context).expect("an element");
        let attrs = self.context_attrs(context, ns, local);
        let quirks = match (self.n(owner).flags & QUIRKS_MASK) >> QUIRKS_SHIFT {
            0 => QuirksMode::NoQuirks,
            1 => QuirksMode::LimitedQuirks,
            _ => QuirksMode::Quirks,
        };
        let form = std::iter::successors(Some(context), |&n| self.parent(n))
            .find(|&n| self.namespace(n) == Some(Namespace::Html) && self.local_name(n) == Some(atom!("form")));
        // the refusals so far were script's: they do not stop this parser
        let (main, cut) = (self.doc, self.cut.take());
        self.doc = owner;
        let sink = Frag { dom: self, doc: NodeId::at(owner), root: None, stopped: false };
        let opts = madcawl_html::Options { scripting, ..Default::default() };
        let mut b = TreeBuilder::new_fragment(sink, opts, FragmentContext { ns, local, attrs: &attrs, form, quirks });
        let mut tok = Tokenizer::new();
        tok.set_state(b.tokenizer_state());
        tok.feed(html);
        tok.end();
        assert_eq!(tok.run(&mut b), Step::Done, "a fragment parser does not pause");
        let Frag { root, stopped, .. } = b.into_sink();
        self.doc = main;
        let complete = !stopped && self.cut.is_none();
        self.cut = cut;
        let root = root.expect("the fragment's root").index();
        while self.n(root).first() != 0 {
            self.insert(fragment.index(), self.n(root).first(), 0);
        }
        if self.n(root).count() == 0 {
            self.zero.push(root);
        }
        Some(Parsed { fragment, complete })
    }

    /// The context's attributes as the tree builder reads them: only an
    /// `annotation-xml` context's decide anything (its `encoding`).
    fn context_attrs(&self, context: NodeId, ns: Namespace, local: Atom) -> Vec<Attribute> {
        if ns != Namespace::MathMl || local != atom!("annotation-xml") {
            return Vec::new();
        }
        let text = |s: &madcawl_str::DomString| {
            let mut b = Vec::new();
            s.write_wtf8(&mut b);
            b
        };
        self.attributes(context)
            .filter(|a| a.ns == AttrNamespace::None)
            .map(|a| Attribute { name: text(&a.local.to_dom_string()), atom: Some(a.local), value: text(a.value) })
            .collect()
    }

    /// An HTML element named `local` whose node document is `doc`, in
    /// no tree; `None` when the DOM holds all it may.
    pub fn new_element_in(&mut self, doc: NodeId, local: Atom) -> Option<NodeId> {
        assert_eq!(self.kind(doc), NodeKind::Document, "a document");
        let cut = self.cut;
        let el = self.element(Namespace::Html, local, doc.index());
        self.cut = cut;
        el.map(NodeId::at)
    }
}
