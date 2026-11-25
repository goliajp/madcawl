//! What the entry list needs to know about a node: its HTML name and
//! attributes as strings, its text, whether it is disabled, and which
//! form owns it.

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{Atom, DomString, atom};

/// A DOM string as UTF-8; lone surrogates become U+FFFD, as converting
/// to scalar values does.
pub(crate) fn string(s: &DomString) -> String {
    if let Some(a) = s.as_ascii_str() {
        return a.to_owned();
    }
    let mut b = Vec::with_capacity(s.len());
    s.write_wtf8(&mut b);
    String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

pub(crate) fn is_html(dom: &Dom, n: NodeId, name: Atom) -> bool {
    dom.namespace(n) == Some(Namespace::Html) && dom.local_name(n) == Some(name)
}

pub(crate) fn attr(dom: &Dom, n: NodeId, a: Atom) -> Option<String> {
    (dom.kind(n) == NodeKind::Element).then(|| dom.attribute(n, a).map(string)).flatten()
}

/// The attribute's value ASCII-lowercased with surrounding whitespace
/// removed, for enumerated attributes.
pub(crate) fn keyword(dom: &Dom, n: NodeId, a: Atom) -> Option<String> {
    attr(dom, n, a).map(|v| v.trim_matches(is_space).to_ascii_lowercase())
}

pub(crate) fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

const INPUT_TYPES: &[&str] = &[
    "hidden", "text", "search", "tel", "url", "email", "password", "date", "month", "week", "time", "datetime-local", "number",
    "range", "color", "checkbox", "radio", "file", "submit", "image", "reset", "button",
];

/// An input's type state; a missing or unknown type is `text`.
pub(crate) fn input_type(dom: &Dom, n: NodeId) -> &'static str {
    let t = keyword(dom, n, atom!("type")).unwrap_or_default();
    INPUT_TYPES.iter().find(|&&k| k == t).copied().unwrap_or("text")
}

/// A button's type state; a missing or unknown type is `submit`.
pub(crate) fn button_type(dom: &Dom, n: NodeId) -> &'static str {
    match keyword(dom, n, atom!("type")).as_deref() {
        Some("reset") => "reset",
        Some("button") => "button",
        _ => "submit",
    }
}

/// A button, or an input in the Submit Button or Image Button state.
pub fn is_submit_button(dom: &Dom, n: NodeId) -> bool {
    (is_html(dom, n, atom!("button")) && button_type(dom, n) == "submit")
        || (is_html(dom, n, atom!("input")) && matches!(input_type(dom, n), "submit" | "image"))
}

/// The text of the node's descendant text nodes, in tree order.
pub(crate) fn text_content(dom: &Dom, n: NodeId) -> String {
    let mut out = String::new();
    for d in dom.descendants(n) {
        if dom.kind(d) == NodeKind::Text
            && let Some(t) = dom.text(d)
        {
            out.push_str(&string(t));
        }
    }
    out
}

pub(crate) fn ancestors(dom: &Dom, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    std::iter::successors(dom.parent(n), |&p| dom.parent(p))
}

/// A form control is disabled by its own `disabled` attribute, or by a
/// disabled fieldset ancestor unless it is inside that fieldset's first
/// legend child.
pub(crate) fn is_disabled(dom: &Dom, n: NodeId) -> bool {
    if dom.attribute(n, atom!("disabled")).is_some() {
        return true;
    }
    let mut child = n;
    for a in ancestors(dom, n) {
        if is_html(dom, a, atom!("fieldset")) && dom.attribute(a, atom!("disabled")).is_some() {
            let first_legend = dom.children(a).find(|&c| is_html(dom, c, atom!("legend")));
            if first_legend != Some(child) {
                return true;
            }
        }
        child = a;
    }
    false
}

/// An option is disabled by its own attribute or by its optgroup's.
pub(crate) fn option_disabled(dom: &Dom, n: NodeId) -> bool {
    dom.attribute(n, atom!("disabled")).is_some()
        || dom.parent(n).is_some_and(|p| is_html(dom, p, atom!("optgroup")) && dom.attribute(p, atom!("disabled")).is_some())
}

/// The root of the node's tree.
pub(crate) fn root(dom: &Dom, n: NodeId) -> NodeId {
    ancestors(dom, n).last().unwrap_or(n)
}

/// Decides for elements of one tree whether `form` owns them: an element
/// with a `form` attribute belongs to the first element of the tree with
/// that ID when that is a form; one without, to the form the parser
/// associated it with, else to its nearest form ancestor.
pub(crate) struct Owner {
    form: NodeId,
    /// the form's ID when the form is the first element with it
    id: Option<String>,
}

impl Owner {
    pub fn new(dom: &Dom, form: NodeId) -> Owner {
        let id = attr(dom, form, atom!("id")).filter(|v| !v.is_empty());
        let first = id.as_ref().and_then(|v| {
            dom.descendants(root(dom, form)).find(|&d| attr(dom, d, atom!("id")).as_deref() == Some(v.as_str()))
        });
        Owner { form, id: id.filter(|_| first == Some(form)) }
    }

    pub fn owns(&self, dom: &Dom, n: NodeId) -> bool {
        if let Some(v) = attr(dom, n, atom!("form")) {
            return self.id.as_deref() == Some(v.as_str());
        }
        if let Some(f) = dom.form_owner(n) {
            return f == self.form;
        }
        ancestors(dom, n).find(|&a| is_html(dom, a, atom!("form"))) == Some(self.form)
    }
}
