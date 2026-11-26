//! Setting attributes after the parser is done, as `setAttribute`
//! does: the value replaced in place, the id and class list following.

use madcawl_html::{AttrNamespace, AttrRef, Namespace};
use madcawl_str::{Atom, DomString, atom};

use crate::attrs::Unit;
use crate::dom::{Dom, NodeId};
use crate::node::{HAS_CLASS, HAS_ID, HAS_STYLE, NodeKind};

impl Dom {
    /// An HTML element named `name` (already lowercased), owned by the
    /// document, in no tree; `None` when the DOM holds all it may. A
    /// refusal from script, here and in the host's other edits, does not
    /// stop a parser building into the DOM: only the parser's own do.
    pub fn new_element(&mut self, name: &[u8]) -> Option<NodeId> {
        let local = self.atoms.hold(name);
        let cut = self.cut;
        let el = self.element(Namespace::Html, local, self.doc);
        self.cut = cut;
        el.map(NodeId::at)
    }

    /// Count `n` bytes for script, if they fit.
    pub(crate) fn take(&mut self, n: usize) -> bool {
        let fits = self.limits.bytes - self.bytes >= n;
        self.bytes += if fits { n } else { 0 };
        fits
    }

    /// Set the no-namespace attribute `local` of element `el` to
    /// `value`, keeping its place when it has one, as `setAttribute`
    /// does; its `id` and class list follow. False once the byte limit
    /// is reached and the value is cut short, or when the id or class
    /// table has no room for a new value and the value is left empty.
    /// A refusal here does not stop a parser building into the DOM.
    pub fn set_attribute(&mut self, el: NodeId, local: Atom, value: &[u8]) -> bool {
        let i = el.index();
        assert_eq!(
            self.kind_of(i),
            NodeKind::Element,
            "attributes go on an element"
        );
        let cut = self.cut;
        let done = self.set_attr(i, local, value);
        self.cut = cut;
        self.notify(crate::tree::Change::Attributes(i));
        done
    }

    /// Remove attribute `k` of element `el`, in the order `attributes`
    /// gives them; its value gives back its bytes, its slot and its table
    /// entries, and the element's id or class list goes with it.
    pub fn remove_attribute(&mut self, el: NodeId, k: usize) {
        let i = el.index();
        let b = self.n(i).attrs;
        let u = self.attrs.remove(b, k);
        self.refund(u.value());
        self.bytes -= 16;
        if u.ns() == AttrNamespace::None {
            let local = u.local();
            if local == atom!("class") {
                self.versions.class += 1;
                let mut was = Vec::new();
                u.value().write_wtf8(&mut was);
                self.tables.drop_class(&was);
                self.attrs.set_classes(b, 0);
                self.m(i).flags &= !HAS_CLASS;
            } else if local == atom!("id") {
                if let Some(id) = self.attrs.id(b) {
                    self.tables.drop_id(id);
                }
                self.attrs.clear_id(b);
                self.m(i).flags &= !HAS_ID;
            } else if local == atom!("style") {
                self.m(i).flags &= !HAS_STYLE;
            }
        }
        u.drop_value();
        self.notify(crate::tree::Change::Attributes(i));
    }

    /// Give the no-namespace attribute `local` of `el` the value `v`,
    /// in the place it has, adding it when it is missing; the value it
    /// replaces gives back its bytes and its table entries.
    pub(crate) fn set_attr(&mut self, el: u32, local: Atom, v: &[u8]) -> bool {
        let b = self.n(el).attrs;
        let at = (b != 0)
            .then(|| {
                self.attrs
                    .attrs(b)
                    .iter()
                    .position(|u| u.ns() == AttrNamespace::None && u.local() == local)
            })
            .flatten();
        let Some(k) = at else {
            let a = AttrRef {
                ns: AttrNamespace::None,
                local,
                prefix: None,
                value: v,
            };
            return self.add_attr(el, a);
        };
        let old = self.attrs.attrs(b)[k];
        self.refund(old.value());
        let (value, classes, id, whole) = self.attr_value(AttrNamespace::None, local, v);
        let old = self
            .attrs
            .replace(b, k, Unit::attr(local, AttrNamespace::None, None, value));
        if let Some(k) = classes {
            let mut was = Vec::new();
            old.value().write_wtf8(&mut was);
            self.tables.drop_class(&was);
            self.attrs.set_classes(b, k);
            self.m(el).flags |= HAS_CLASS;
        }
        old.drop_value();
        if local == atom!("id") {
            if let Some(was) = self.attrs.id(b) {
                self.tables.drop_id(was);
            }
            match id {
                Some(id) => {
                    self.attrs.set_id(b, id);
                    self.m(el).flags |= HAS_ID;
                }
                None => {
                    self.attrs.clear_id(b);
                    self.m(el).flags &= !HAS_ID;
                }
            }
        }
        whole
    }

    /// The stored form of the value `v` of the attribute `local` in
    /// `ns`, counted: its string, its class list number for `class`, its
    /// atom for a non-empty `id`, and whether it was stored whole. A
    /// new id or class value the tables have no room for is stored
    /// empty.
    pub(crate) fn attr_value(
        &mut self,
        ns: AttrNamespace,
        local: Atom,
        v: &[u8],
    ) -> (DomString, Option<u32>, Option<Atom>, bool) {
        let plain = ns == AttrNamespace::None;
        let kept = self.admit(v);
        let whole = kept.len() == v.len();
        // a pair of surrogate halves of three bytes each is stored as one
        // code point of four, and the tables key on and give back what is
        // stored
        let joined = crate::text::split_pair(kept).then(|| {
            let mut b = Vec::with_capacity(kept.len());
            DomString::from_wtf8(kept).write_wtf8(&mut b);
            b
        });
        if let Some(b) = &joined {
            self.bytes -= kept.len() - b.len();
        }
        let kept = joined.as_deref().unwrap_or(kept);
        if plain && local == atom!("class") {
            self.versions.class += 1;
            if let Some((s, k)) = self.tables.class(kept) {
                return (s, Some(k), None, whole);
            }
            self.bytes -= kept.len();
            return (DomString::empty(), Some(0), None, false);
        }
        if plain && local == atom!("id") && !kept.is_empty() {
            if let Some(id) = self.tables.id(kept) {
                return (self.short.get(kept), None, Some(id), whole);
            }
            self.bytes -= kept.len();
            return (DomString::empty(), None, None, false);
        }
        (self.short.get(kept), None, None, whole)
    }
}
