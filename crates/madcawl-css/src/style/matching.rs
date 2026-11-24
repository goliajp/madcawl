//! Selector matching against a `madcawl_dom::Dom`: right to left from
//! the element, as a selector's components are stored. A selector that
//! ends in a combinator is relative: in a nested rule the combinator
//! joins it to an element the parent rule matches, in `:has()` to the
//! element `:has()` is on.

use std::borrow::Cow;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::{AttrNamespace, Namespace as ElementNs};
use madcawl_str::{Atom, DomString};

use crate::Name;
use crate::selector::{
    AttrOp, AttrSelector, CaseFlag, Combinator, Component, Namespace, Nth, NthKind, SelectorList,
};

use super::rules::{Entry, NONE, Rules, Sel};

pub(crate) struct Matcher<'r, 'a> {
    pub dom: &'r Dom,
    pub rules: &'r Rules<'a>,
    /// Ids and classes compare ASCII case-insensitively
    pub quirks: bool,
}

pub(crate) fn text(s: &DomString) -> Cow<'_, str> {
    match s.as_ascii_str() {
        Some(a) => Cow::Borrowed(a),
        None => {
            let mut v = Vec::new();
            s.write_wtf8(&mut v);
            Cow::Owned(String::from_utf8_lossy(&v).into_owned())
        }
    }
}

fn atom_text(a: Atom) -> String {
    let mut v = Vec::new();
    a.write_wtf8(&mut v);
    String::from_utf8_lossy(&v).into_owned()
}

/// HTML attributes whose values selectors compare ASCII
/// case-insensitively on HTML elements (HTML, "case-sensitivity of
/// selectors"), sorted.
const CASELESS: &[&str] = &[
    "accept",
    "accept-charset",
    "align",
    "alink",
    "axis",
    "bgcolor",
    "charset",
    "checked",
    "clear",
    "codetype",
    "color",
    "compact",
    "declare",
    "defer",
    "dir",
    "direction",
    "disabled",
    "enctype",
    "face",
    "frame",
    "hreflang",
    "http-equiv",
    "lang",
    "language",
    "link",
    "media",
    "method",
    "multiple",
    "nohref",
    "noresize",
    "noshade",
    "nowrap",
    "readonly",
    "rel",
    "rev",
    "rules",
    "scope",
    "scrolling",
    "selected",
    "shape",
    "target",
    "text",
    "type",
    "valign",
    "valuetype",
    "vlink",
];

impl Matcher<'_, '_> {
    pub fn entry(&self, e: &Entry<'_>, el: NodeId) -> bool {
        match e.sel {
            Sel::Complex(c) => self.from(e.sheet, c.comps, 0, el, e.parent, None),
            Sel::Parent => self.parent_matches(e.sheet, e.parent, el),
        }
    }

    pub(crate) fn element(&self, n: Option<NodeId>) -> Option<NodeId> {
        n.filter(|&x| self.dom.kind(x) == NodeKind::Element)
    }

    pub(crate) fn parent_element(&self, el: NodeId) -> Option<NodeId> {
        self.element(self.dom.parent(el))
    }

    pub(crate) fn prev_element(&self, el: NodeId) -> Option<NodeId> {
        let mut x = self.dom.previous_sibling(el);
        while let Some(n) = x {
            if self.dom.kind(n) == NodeKind::Element {
                return Some(n);
            }
            x = self.dom.previous_sibling(n);
        }
        None
    }

    pub(crate) fn next_element(&self, el: NodeId) -> Option<NodeId> {
        let mut x = self.dom.next_sibling(el);
        while let Some(n) = x {
            if self.dom.kind(n) == NodeKind::Element {
                return Some(n);
            }
            x = self.dom.next_sibling(n);
        }
        None
    }

    pub(crate) fn is_html(&self, el: NodeId) -> bool {
        self.dom.namespace(el) == Some(ElementNs::Html)
    }

    pub(crate) fn is_root(&self, el: NodeId) -> bool {
        self.dom
            .parent(el)
            .is_some_and(|p| self.dom.kind(p) == NodeKind::Document)
    }

    fn parent_matches(&self, k: u32, parent: u32, el: NodeId) -> bool {
        if parent == NONE {
            // `&` outside any style rule is `:scope`
            return self.is_root(el);
        }
        let p = &self.rules.parents[parent as usize];
        p.sels
            .iter()
            .any(|c| self.from(k, c.comps, 0, el, p.parent, None))
    }

    fn from(
        &self,
        k: u32,
        comps: &[Component],
        i: usize,
        el: NodeId,
        parent: u32,
        anchor: Option<NodeId>,
    ) -> bool {
        let mut j = i;
        while let Some(c) = comps.get(j) {
            if let Component::Combinator(_) = c {
                break;
            }
            if !self.simple(k, c, el, parent) {
                return false;
            }
            j += 1;
        }
        let Some(&Component::Combinator(comb)) = comps.get(j) else {
            return true;
        };
        let last = j + 1 == comps.len();
        let next = |x: NodeId| {
            if !last {
                self.from(k, comps, j + 1, x, parent, anchor)
            } else if let Some(a) = anchor {
                x == a
            } else {
                self.parent_matches(k, parent, x)
            }
        };
        let step = |x: NodeId| match comb {
            Combinator::Child | Combinator::Descendant => self.parent_element(x),
            Combinator::NextSibling | Combinator::SubsequentSibling => self.prev_element(x),
        };
        let once = matches!(comb, Combinator::Child | Combinator::NextSibling);
        let mut x = step(el);
        while let Some(n) = x {
            if next(n) {
                return true;
            }
            if once {
                return false;
            }
            x = step(n);
        }
        false
    }

    fn list(&self, k: u32, l: &SelectorList, el: NodeId, parent: u32) -> bool {
        l.iter()
            .any(|s| self.from(k, &s.components, 0, el, parent, None))
    }

    fn atom(&self, k: u32, n: Name) -> Atom {
        self.rules.sheets[k as usize].atoms[n.index()]
    }

    fn name(&self, k: u32, n: Name) -> &str {
        self.rules.sheets[k as usize].sheet.names().get(n)
    }

    fn ns_ok(&self, k: u32, ns: Namespace, el: NodeId) -> bool {
        let url = match ns {
            Namespace::Implicit(None) | Namespace::Any => return true,
            Namespace::None => return false,
            Namespace::Implicit(Some(url)) | Namespace::Prefix { url, .. } => url,
        };
        let want = self.rules.sheets[k as usize].ns[url.index()];
        want.is_some() && self.dom.namespace(el) == want
    }

    fn simple(&self, k: u32, c: &Component, el: NodeId, parent: u32) -> bool {
        let dom = self.dom;
        match c {
            Component::Universal(ns) => self.ns_ok(k, *ns, el),
            Component::Type { ns, name, lower } => {
                let n = if self.is_html(el) { *lower } else { *name };
                dom.local_name(el) == Some(self.atom(k, n)) && self.ns_ok(k, *ns, el)
            }
            Component::Id(n) => dom.element_id(el).is_some_and(|id| {
                if self.quirks {
                    atom_text(id).eq_ignore_ascii_case(self.name(k, *n))
                } else {
                    id == self.atom(k, *n)
                }
            }),
            Component::Class(n) => {
                let (_, classes) = dom.class_list(el);
                if self.quirks {
                    let want = self.name(k, *n);
                    classes
                        .iter()
                        .any(|&c| atom_text(c).eq_ignore_ascii_case(want))
                } else {
                    classes.contains(&self.atom(k, *n))
                }
            }
            Component::Attribute(a) => self.attr(k, a, el),
            Component::PseudoClass(p) => self.pseudo(*p, el),
            Component::Nth(n) => self.nth(k, n, el, parent),
            Component::Not(l) => !self.list(k, l, el, parent),
            Component::Is(l) | Component::Where(l) => self.list(k, l, el, parent),
            Component::Has(l) => self.has(k, l, el, parent),
            Component::Lang(ranges) => self.lang(k, ranges, el),
            Component::Dir(d) => self.dir(el) == *d,
            Component::PseudoElement(_) => false,
            Component::Nesting => self.parent_matches(k, parent, el),
            Component::Combinator(_) => unreachable!("compounds hold no combinator"),
        }
    }

    fn attr(&self, k: u32, a: &AttrSelector, el: NodeId) -> bool {
        let html = self.is_html(el);
        let want = self.atom(k, if html { a.lower } else { a.name });
        self.dom.attributes(el).any(|at| {
            let ns = match a.ns {
                Namespace::Implicit(_) | Namespace::None => at.ns == AttrNamespace::None,
                Namespace::Any => true,
                Namespace::Prefix { url, .. } => at.ns.url() == Some(self.name(k, url)),
            };
            if at.local != want || !ns {
                return false;
            }
            let Some((op, val, flag)) = a.op else {
                return true;
            };
            let caseless = match flag {
                CaseFlag::Insensitive => true,
                CaseFlag::Sensitive => false,
                CaseFlag::Default => {
                    html && at.ns == AttrNamespace::None
                        && CASELESS.binary_search(&self.name(k, a.lower)).is_ok()
                }
            };
            value_matches(op, &text(at.value), self.name(k, val), caseless)
        })
    }

    fn nth(&self, k: u32, n: &Nth, el: NodeId, parent: u32) -> bool {
        let of_type = matches!(n.kind, NthKind::OfType | NthKind::LastOfType);
        let (ns, local) = (self.dom.namespace(el), self.dom.local_name(el));
        let counts = |x: NodeId| {
            if of_type {
                self.dom.local_name(x) == local && self.dom.namespace(x) == ns
            } else {
                n.of.as_ref().is_none_or(|l| self.list(k, l, x, parent))
            }
        };
        if !counts(el) {
            return false;
        }
        let back = matches!(n.kind, NthKind::Child | NthKind::OfType);
        let mut i: i64 = 1;
        let mut x = if back {
            self.prev_element(el)
        } else {
            self.next_element(el)
        };
        while let Some(s) = x {
            i += counts(s) as i64;
            x = if back {
                self.prev_element(s)
            } else {
                self.next_element(s)
            };
        }
        let (a, b) = (i64::from(n.a), i64::from(n.b));
        if a == 0 {
            i == b
        } else {
            (i - b) % a == 0 && (i - b) / a >= 0
        }
    }

    fn has(&self, k: u32, l: &SelectorList, el: NodeId, parent: u32) -> bool {
        l.iter().any(|s| {
            let siblings = matches!(
                s.components.last(),
                Some(Component::Combinator(
                    Combinator::NextSibling | Combinator::SubsequentSibling
                ))
            );
            let test = |x: NodeId| {
                self.dom.kind(x) == NodeKind::Element
                    && self.from(k, &s.components, 0, x, parent, Some(el))
            };
            if !siblings {
                return self.dom.descendants(el).any(test);
            }
            let mut sib = self.next_element(el);
            while let Some(x) = sib {
                if test(x) || self.dom.descendants(x).any(test) {
                    return true;
                }
                sib = self.next_element(x);
            }
            false
        })
    }

    fn lang(&self, k: u32, ranges: &[Name], el: NodeId) -> bool {
        let mut x = Some(el);
        let mut lang = None;
        while let Some(n) = x {
            let found = self.dom.attributes(n).find(|a| {
                a.local == madcawl_str::atom!("lang")
                    && (a.ns == AttrNamespace::Xml
                        || a.ns == AttrNamespace::None && self.is_html(n))
            });
            if let Some(a) = found {
                lang = Some(text(a.value).to_ascii_lowercase());
                break;
            }
            x = self.parent_element(n);
        }
        let Some(lang) = lang else { return false };
        ranges.iter().any(|&r| {
            let r = self.name(k, r).to_ascii_lowercase();
            if r == "*" {
                return !lang.is_empty();
            }
            let r = r.strip_prefix("*-").unwrap_or(&r);
            lang == r || lang.starts_with(r) && lang.as_bytes().get(r.len()) == Some(&b'-')
        })
    }
}

fn value_matches(op: AttrOp, have: &str, want: &str, caseless: bool) -> bool {
    let (h, w): (Cow<'_, str>, Cow<'_, str>) = if caseless {
        (
            have.to_ascii_lowercase().into(),
            want.to_ascii_lowercase().into(),
        )
    } else {
        (have.into(), want.into())
    };
    let (h, w) = (&*h, &*w);
    match op {
        AttrOp::Equals => h == w,
        AttrOp::Includes => {
            !w.is_empty()
                && !w.contains(|c: char| c.is_ascii_whitespace())
                && h.split(|c: char| c.is_ascii_whitespace()).any(|p| p == w)
        }
        AttrOp::DashMatch => h == w || h.starts_with(w) && h.as_bytes().get(w.len()) == Some(&b'-'),
        AttrOp::Prefix => !w.is_empty() && h.starts_with(w),
        AttrOp::Suffix => !w.is_empty() && h.ends_with(w),
        AttrOp::Substring => !w.is_empty() && h.contains(w),
    }
}
