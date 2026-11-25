//! What a click does on a page without script: the activation behaviour
//! of the element hit or its nearest ancestor that has one (HTML 6.2,
//! "activation behavior"), and which controls take typed text.

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_form::FormOwners;
use madcawl_str::{Atom, atom};

use crate::html_table::is_html;
use crate::text::{attr_string, collapsed, text_content};

/// An attribute's value as text.
pub fn attr(dom: &Dom, n: NodeId, a: Atom) -> Option<String> {
    attr_string(dom, n, a)
}

/// The text inside `n`, white space collapsed.
pub fn text_of(dom: &Dom, n: NodeId) -> String {
    collapsed(&text_content(dom, n))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// A link: follow its `href`
    Follow(NodeId),
    /// A submit button: submit its form with it as the submitter
    Submit { form: NodeId, submitter: NodeId },
    /// A checkbox: flip it
    Toggle(NodeId),
    /// A radio button: check it, clearing the others of its group
    Choose(NodeId),
    /// A reset button: its form's controls back to their markup
    Reset(NodeId),
    /// A control that takes focus and nothing else
    Focus(NodeId),
    Nothing,
}

fn keyword(dom: &Dom, n: NodeId, a: Atom) -> String {
    attr_string(dom, n, a).unwrap_or_default().trim().to_ascii_lowercase()
}

/// An input's type as its attribute says, `text` when missing or unknown
/// to this list.
pub fn input_type(dom: &Dom, n: NodeId) -> String {
    let t = keyword(dom, n, atom!("type"));
    const KNOWN: [&str; 22] = [
        "hidden", "text", "search", "tel", "url", "email", "password", "date", "month", "week", "time",
        "datetime-local", "number", "range", "color", "checkbox", "radio", "file", "submit", "image",
        "reset", "button",
    ];
    if KNOWN.contains(&t.as_str()) { t } else { "text".into() }
}

/// A control text is typed into: a text-like input or a textarea.
pub fn is_editable(dom: &Dom, n: NodeId) -> bool {
    if is_html(dom, n, atom!("textarea")) {
        return true;
    }
    const TEXT_LIKE: [&str; 13] = [
        "text", "search", "tel", "url", "email", "password", "number", "date", "month", "week", "time",
        "datetime-local", "color",
    ];
    is_html(dom, n, atom!("input")) && TEXT_LIKE.contains(&input_type(dom, n).as_str())
}

pub fn is_password(dom: &Dom, n: NodeId) -> bool {
    is_html(dom, n, atom!("input")) && input_type(dom, n) == "password"
}

/// A submit button: `input` of type submit or image, a `button` whose
/// type is submit or missing or unknown.
pub fn is_submit(dom: &Dom, n: NodeId) -> bool {
    if is_html(dom, n, atom!("input")) {
        return matches!(input_type(dom, n).as_str(), "submit" | "image");
    }
    is_html(dom, n, atom!("button")) && !matches!(keyword(dom, n, atom!("type")).as_str(), "reset" | "button")
}

fn is_reset(dom: &Dom, n: NodeId) -> bool {
    let input = is_html(dom, n, atom!("input")) && input_type(dom, n) == "reset";
    input || is_html(dom, n, atom!("button")) && keyword(dom, n, atom!("type")) == "reset"
}

/// The form a control belongs to, by the rule submissions use: its
/// `form` attribute's form, else the form the parser associated it with,
/// else its nearest `form` ancestor.
pub fn form_owner(dom: &Dom, n: NodeId) -> Option<NodeId> {
    madcawl_form::form_owner(dom, n)
}

/// The control a label is for: the element its `for` names, else the
/// first labelable element inside it.
pub fn labeled_control(dom: &Dom, label: NodeId) -> Option<NodeId> {
    let labelable = |c: NodeId| {
        [atom!("input"), atom!("select"), atom!("textarea"), atom!("button")].iter().any(|&a| is_html(dom, c, a))
            && !(is_html(dom, c, atom!("input")) && input_type(dom, c) == "hidden")
    };
    match attr_string(dom, label, atom!("for")) {
        Some(id) => dom
            .descendants(dom.document())
            .find(|&c| attr_string(dom, c, atom!("id")).as_deref() == Some(id.as_str()))
            .filter(|&c| labelable(c)),
        None => dom.descendants(label).find(|&c| labelable(c)),
    }
}

/// The behaviour of the element itself, if it has one.
fn own(dom: &Dom, n: NodeId) -> Option<Activation> {
    if dom.kind(n) != NodeKind::Element {
        return None;
    }
    let link = is_html(dom, n, atom!("a")) || is_html(dom, n, atom!("area"));
    if link && dom.attribute(n, atom!("href")).is_some() {
        return Some(Activation::Follow(n));
    }
    if is_html(dom, n, atom!("input")) {
        return Some(match input_type(dom, n).as_str() {
            "checkbox" => Activation::Toggle(n),
            "radio" => Activation::Choose(n),
            "hidden" => return None,
            _ if is_submit(dom, n) => submit(dom, n),
            _ if is_reset(dom, n) => form_owner(dom, n).map_or(Activation::Nothing, Activation::Reset),
            _ => Activation::Focus(n),
        });
    }
    if is_html(dom, n, atom!("button")) {
        return Some(match () {
            _ if is_submit(dom, n) => submit(dom, n),
            _ if is_reset(dom, n) => form_owner(dom, n).map_or(Activation::Nothing, Activation::Reset),
            _ => Activation::Nothing,
        });
    }
    if is_html(dom, n, atom!("select")) || is_html(dom, n, atom!("textarea")) {
        return Some(Activation::Focus(n));
    }
    None
}

fn submit(dom: &Dom, n: NodeId) -> Activation {
    match form_owner(dom, n) {
        Some(form) => Activation::Submit { form, submitter: n },
        None => Activation::Nothing,
    }
}

/// What clicking `n` does: the behaviour of `n` or of its nearest
/// ancestor that has one; a label's is its control's, unless the click
/// landed inside that control already.
pub fn activation(dom: &Dom, n: NodeId) -> Activation {
    let mut up = Some(n);
    while let Some(e) = up {
        if let Some(a) = own(dom, e) {
            return a;
        }
        if is_html(dom, e, atom!("label")) {
            return labeled_control(dom, e)
                .filter(|&c| !dom.is_inclusive_ancestor_of(c, n))
                .and_then(|c| own(dom, c))
                .unwrap_or(Activation::Nothing);
        }
        up = dom.parent(e);
    }
    Activation::Nothing
}

/// The radio buttons of `n`'s group: same name, same form owner, in the
/// document.
pub fn radio_group(dom: &Dom, n: NodeId) -> Vec<NodeId> {
    let Some(name) = attr_string(dom, n, atom!("name")).filter(|s| !s.is_empty()) else {
        return vec![n];
    };
    let forms = FormOwners::new(dom, dom.document());
    let form = forms.owner(dom, n);
    dom.descendants(dom.document())
        .filter(|&r| {
            is_html(dom, r, atom!("input"))
                && input_type(dom, r) == "radio"
                && attr_string(dom, r, atom!("name")).as_deref() == Some(name.as_str())
                && forms.owner(dom, r) == form
        })
        .collect()
}

/// The options of a select, in tree order, optgroups' included.
pub fn options(dom: &Dom, select: NodeId) -> Vec<NodeId> {
    dom.descendants(select).filter(|&o| is_html(dom, o, atom!("option"))).collect()
}

/// The controls a form owns, in tree order: the ones its submission
/// reads.
pub fn form_controls(dom: &Dom, form: NodeId) -> Vec<NodeId> {
    madcawl_form::owned_controls(dom, form)
}
