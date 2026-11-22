//! A customizable `<select>` shows its selected option's content in its
//! `<selectedcontent>` element: when the parser has put every child into
//! an option, the option's children are cloned into it (HTML, "maybe
//! clone an option into selectedcontent").

use madcawl_str::atom;

use crate::dom::Dom;
use crate::node::NodeKind;

impl Dom {
    pub(crate) fn clone_into_selectedcontent(&mut self, option: u32) {
        let mut select = self.n(option).parent;
        while select != 0 && !self.is_html(select, atom!("select")) {
            select = self.n(select).parent;
        }
        if select == 0 || self.has_attr(select, atom!("multiple")) {
            return;
        }
        let (first, target) = self.select_firsts(select);
        if target == 0 || !self.is_selected(select, option, first) {
            return;
        }
        // a selectedcontent inside the option, or around it, is not
        // filled from it
        if self.is_inclusive_ancestor(option, target) || self.is_inclusive_ancestor(target, option)
        {
            return;
        }
        // the old content is only taken out: the parser may still hold
        // handles to it (`Dom::reclaim` frees it once parsing is over)
        loop {
            let c = self.n(target).first();
            if c == 0 {
                break;
            }
            self.detach(c);
        }
        let doc = self.doc_of(target);
        let mut c = self.n(option).first();
        while c != 0 {
            let next = self.n(c).next;
            let Some(copy) = self.deep_clone(c, doc) else {
                return;
            };
            self.insert(target, copy, 0);
            c = next;
        }
    }

    /// The option has `selected`, or it is the select's first option and
    /// no option has `selected`.
    fn is_selected(&self, select: u32, option: u32, first: u32) -> bool {
        if self.has_attr(option, atom!("selected")) {
            return true;
        }
        first == option
            && self
                .find_below(select, |d, x| {
                    d.is_html(x, atom!("option")) && d.has_attr(x, atom!("selected"))
                })
                .is_none()
    }

    /// Room for `n` more nodes made by the DOM itself, leaving the
    /// parser's headroom untouched.
    fn clone_room(&mut self, n: u32) -> bool {
        if self.arena.count() + crate::HEADROOM + n > self.limits.nodes {
            self.cut = Some(crate::Refused::Nodes);
            return false;
        }
        true
    }

    /// A copy of `x` and everything below it, owned by `doc`; `None`
    /// when the node limit stops it before `x` itself is copied.
    fn deep_clone(&mut self, x: u32, doc: u32) -> Option<u32> {
        let kind = self.kind_of(x);
        if !self.clone_room(3) {
            return None;
        }
        let copy = match kind {
            NodeKind::Element => self.clone_element(x, doc)?,
            NodeKind::Text | NodeKind::Comment | NodeKind::ProcessingInstruction => {
                let s = self.char_data(x).clone();
                let c = self.new_char_data(kind, doc, s)?;
                self.m(c).name = self.n(x).name;
                c
            }
            NodeKind::Document | NodeKind::Doctype | NodeKind::Fragment => {
                unreachable!("only elements and character data are below an option")
            }
        };
        self.clone_children(x, copy, doc);
        Some(copy)
    }

    fn clone_children(&mut self, from: u32, to: u32, doc: u32) {
        if !self.kind_of(from).is_container() {
            return;
        }
        let mut c = self.n(from).first();
        while c != 0 {
            let Some(cc) = self.deep_clone(c, doc) else {
                return;
            };
            self.insert(to, cc, 0);
            c = self.n(c).next;
        }
    }

    fn clone_element(&mut self, x: u32, doc: u32) -> Option<u32> {
        let id = crate::NodeId::at(x);
        let ns = self.namespace(id).expect("an element");
        let local = self.local_name(id).expect("an element");
        let el = self.element(ns, local, doc)?;
        let from = self.n(x).attrs;
        if from != 0 {
            let n = self.attrs.attrs(from).len();
            let mut b = if self.charge(16) {
                self.attrs.alloc(n)
            } else {
                return Some(el);
            };
            for k in 0..n {
                if !self.charge(16) {
                    break;
                }
                let u = self.attrs.attrs(from)[k];
                let value = u.value().clone();
                b = self.attrs.push(
                    b,
                    crate::attrs::Unit::attr(u.local(), u.ns(), u.prefix(), value),
                );
            }
            if let Some(id) = self.attrs.id(from) {
                self.attrs.set_id(b, id);
            }
            let classes = self.attrs.classes(from);
            self.attrs.set_classes(b, classes);
            let has = self.n(x).flags
                & (crate::node::HAS_ID | crate::node::HAS_CLASS | crate::node::HAS_STYLE);
            let n = self.m(el);
            n.attrs = b;
            n.flags |= has;
        }
        if let (Some(src), Some(dst)) = (self.contents(id), self.contents(crate::NodeId::at(el))) {
            let inert = self.doc_of(dst.index());
            self.clone_children(src.index(), dst.index(), inert);
        }
        Some(el)
    }
}
