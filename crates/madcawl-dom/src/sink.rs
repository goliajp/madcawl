//! Creating nodes, and the tree builder's sink.

use madcawl_html::{AttrNamespace, AttrRef, Attrs, Namespace, ParseError, QuirksMode, TreeSink};
use madcawl_str::{Atom, DomString, atom};

use crate::attrs::Unit;
use crate::dom::{Dom, NodeId};
use crate::node::{
    HAS_CLASS, HAS_ID, HAS_STYLE, INERT, NS_SHIFT, NodeKind, QUIRKS_MASK, QUIRKS_SHIFT,
};
use crate::tree::Change;

const FULL: &str = "the parser stops while there is room for its nodes";

impl Dom {
    /// An element owned by `owner`; a template gets its contents.
    pub(crate) fn element(&mut self, ns: Namespace, local: Atom, owner: u32) -> Option<u32> {
        let template = ns == Namespace::Html && local == atom!("template");
        // a template, its contents and maybe their owner document: all
        // or none
        if template && self.arena.count() + 3 > self.limits.nodes {
            self.cut = Some(crate::Refused::Nodes);
            return None;
        }
        let el = self.new_node(NodeKind::Element, owner)?;
        self.atoms.keep(local);
        let ns_bits = match ns {
            Namespace::Html => 0,
            Namespace::Svg => 1,
            Namespace::MathMl => 2,
        };
        let n = self.m(el);
        n.name = local.index();
        n.flags |= ns_bits << NS_SHIFT;
        if template {
            let inert = self.inert_of(owner).expect("room checked");
            let contents = self
                .new_node(NodeKind::Fragment, inert)
                .expect("room checked");
            self.rare_mut(el).contents = contents;
            self.inc(contents);
        }
        if ns == Namespace::Html && local == atom!("select") {
            self.rare_mut(el).select_known = true;
        }
        Some(el)
    }

    /// The template contents owner of document `doc`.
    fn inert_of(&mut self, doc: u32) -> Option<u32> {
        if self.n(doc).flags & INERT != 0 {
            return Some(doc);
        }
        let have = self.rare[self.n(doc).rare as usize].inert;
        if have != 0 {
            return Some(have);
        }
        let inert = self.new_document(true)?;
        self.rare_mut(doc).inert = inert;
        self.inc(inert);
        Some(inert)
    }

    /// Add one attribute; false once the byte limit is reached, when
    /// it is left out or its value is cut short.
    pub(crate) fn add_attr(&mut self, el: u32, a: AttrRef<'_>) -> bool {
        let mut b = self.n(el).attrs;
        let room = if b == 0 { 32 } else { 16 };
        if !self.charge(room) {
            return false;
        }
        if b == 0 {
            b = self.attrs.alloc(1);
        }
        self.atoms.keep(a.local);
        let (value, classes, id, whole) = self.attr_value(a.ns, a.local, a.value);
        b = self
            .attrs
            .push(b, Unit::attr(a.local, a.ns, a.prefix, value));
        let mut flags = 0;
        if let Some(k) = classes {
            self.attrs.set_classes(b, k);
            flags |= HAS_CLASS;
        }
        if let Some(id) = id {
            self.attrs.set_id(b, id);
            flags |= HAS_ID;
        }
        if a.ns == AttrNamespace::None && a.local == atom!("style") {
            flags |= HAS_STYLE;
        }
        let n = self.m(el);
        n.attrs = b;
        n.flags |= flags;
        whole
    }

    fn has_attr_ns(&self, el: u32, ns: AttrNamespace, local: Atom) -> bool {
        self.attributes(NodeId::at(el))
            .any(|x| x.ns == ns && x.local == local)
    }

    fn string(&mut self, s: &[u8]) -> DomString {
        let s = self.admit(s);
        DomString::from_wtf8(s)
    }
}

impl TreeSink for Dom {
    type Handle = NodeId;

    fn document(&self) -> NodeId {
        NodeId::at(self.doc)
    }

    fn create_element(&mut self, ns: Namespace, local: Atom, attrs: Attrs<'_>) -> NodeId {
        let el = self.element(ns, local, self.doc).expect(FULL);
        // 16 bytes for the block's head, 16 for each attribute
        if !attrs.is_empty() && self.charge(16) {
            self.m(el).attrs = self.attrs.alloc(attrs.len());
        }
        // past the byte limit the rest is left out; a value the id or
        // class table refuses leaves only itself out
        for a in attrs.iter() {
            if !self.add_attr(el, a) && self.cut.is_some() {
                break;
            }
        }
        NodeId::at(el)
    }

    fn create_comment(&mut self, data: &[u8]) -> NodeId {
        let s = self.string(data);
        NodeId::at(
            self.new_char_data(NodeKind::Comment, self.doc, s)
                .expect(FULL),
        )
    }

    fn create_processing_instruction(&mut self, target: &[u8], data: &[u8]) -> NodeId {
        let target = self.admit(target);
        let name = self.atoms.hold(target);
        let s = self.string(data);
        let pi = self
            .new_char_data(NodeKind::ProcessingInstruction, self.doc, s)
            .expect(FULL);
        self.m(pi).name = name.index();
        NodeId::at(pi)
    }

    fn append_doctype(&mut self, name: &[u8], public_id: &[u8], system_id: &[u8]) {
        let strings = [
            self.string(name),
            self.string(public_id),
            self.string(system_id),
        ];
        let d = self.new_node(NodeKind::Doctype, self.doc).expect(FULL);
        self.rare_mut(d).doctype = Some(Box::new(strings));
        self.insert(self.doc, d, 0);
    }

    fn append(&mut self, parent: NodeId, child: NodeId) {
        self.insert(parent.index(), child.index(), 0);
    }

    fn insert_before(&mut self, parent: NodeId, child: NodeId, before: NodeId) {
        self.insert(parent.index(), child.index(), before.index());
    }

    fn insert_text(&mut self, parent: NodeId, before: Option<NodeId>, text: &[u8]) {
        self.add_text(parent.index(), before.map_or(0, NodeId::index), text);
    }

    fn remove(&mut self, node: NodeId) {
        self.detach(node.index());
    }

    fn reparent_children(&mut self, from: NodeId, to: NodeId) {
        let (from, to) = (from.index(), to.index());
        assert!(self.kind_of(from).is_container(), "children of a container");
        loop {
            let c = self.n(from).first();
            if c == 0 {
                break;
            }
            self.insert(to, c, 0);
        }
    }

    fn parent(&self, node: NodeId) -> Option<NodeId> {
        Dom::parent(self, node)
    }

    fn add_attrs_if_missing(&mut self, element: NodeId, attrs: Attrs<'_>) {
        let el = element.index();
        assert_eq!(
            self.kind_of(el),
            NodeKind::Element,
            "attributes go on an element"
        );
        for a in attrs.iter() {
            if !self.has_attr_ns(el, a.ns, a.local) && !self.add_attr(el, a) && self.cut.is_some() {
                break;
            }
        }
        self.notify(Change::Attributes(el));
    }

    fn template_contents(&mut self, template: NodeId) -> NodeId {
        self.contents(template).expect("a template element")
    }

    fn set_quirks_mode(&mut self, mode: QuirksMode) {
        let bits = match mode {
            QuirksMode::NoQuirks => 0,
            QuirksMode::LimitedQuirks => 1,
            QuirksMode::Quirks => 2,
        };
        let doc = self.doc;
        let n = self.m(doc);
        n.flags = n.flags & !QUIRKS_MASK | bits << QUIRKS_SHIFT;
    }

    fn associate_form(&mut self, element: NodeId, form: NodeId, intended_parent: NodeId) {
        if self.root_of(intended_parent.index()) == self.root_of(form.index()) {
            self.rare_mut(element.index()).form = form.index();
        }
    }

    fn children_finished(&mut self, element: NodeId) {
        if self.is_html(element.index(), atom!("option")) {
            self.clone_into_selectedcontent(element.index());
        }
    }

    fn parse_error(&mut self, error: ParseError) {
        if error == ParseError::DocumentTooLarge {
            self.stopped = true;
        }
    }

    fn is_full(&self) -> bool {
        self.refused().is_some()
    }
}
