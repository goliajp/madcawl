//! Constructing the entry list (HTML, 4.10.21.4): the submittable
//! elements the form owns, in tree order, each giving zero or more
//! name-value entries.

use crate::node::{Owner, attr, input_type, is_disabled, is_html, is_space, option_disabled, root, text_content};
use crate::value::{directionality, sanitize};
use crate::{Controls, Entry, FileValue, Value};
use madcawl_dom::{Dom, NodeId};
use madcawl_str::atom;

/// The only encoding submissions use; `accept-charset` is not consulted.
pub const CHARSET: &str = "UTF-8";

fn text(name: &str, value: impl Into<String>) -> Entry {
    Entry { name: name.to_owned(), value: Value::Text(value.into()) }
}

fn checked(dom: &Dom, n: NodeId, c: &dyn Controls) -> bool {
    c.checked(n).unwrap_or_else(|| dom.attribute(n, atom!("checked")).is_some())
}

/// The options of a select: its option children and those of its
/// optgroup children, in tree order.
fn options(dom: &Dom, select: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    for c in dom.children(select) {
        if is_html(dom, c, atom!("option")) {
            out.push(c);
        } else if is_html(dom, c, atom!("optgroup")) {
            out.extend(dom.children(c).filter(|&o| is_html(dom, o, atom!("option"))));
        }
    }
    out
}

/// The selected options. A select that shows one option at a time has
/// exactly one when any option is not disabled: the last one marked
/// selected, else the first that is not disabled.
fn selected(dom: &Dom, select: NodeId, c: &dyn Controls) -> Vec<NodeId> {
    let opts = options(dom, select);
    let is_sel = |&o: &NodeId| c.selected(o).unwrap_or_else(|| dom.attribute(o, atom!("selected")).is_some());
    let multiple = dom.attribute(select, atom!("multiple")).is_some();
    if multiple {
        return opts.into_iter().filter(is_sel).collect();
    }
    let size = attr(dom, select, atom!("size")).and_then(|s| s.trim_matches(is_space).parse::<u32>().ok()).unwrap_or(1);
    match opts.iter().rev().find(|o| is_sel(o)) {
        Some(&o) => vec![o],
        None if size <= 1 => opts.into_iter().find(|&o| !option_disabled(dom, o)).into_iter().collect(),
        None => Vec::new(),
    }
}

fn option_value(dom: &Dom, o: NodeId) -> String {
    attr(dom, o, atom!("value")).unwrap_or_else(|| {
        let t = text_content(dom, o);
        t.split(is_space).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
    })
}

/// The radio buttons that count as checked: in each group (same name,
/// same form) only the last checked one in tree order.
fn radio_winners(dom: &Dom, fields: &[NodeId], c: &dyn Controls) -> Vec<NodeId> {
    let mut last: Vec<(String, NodeId)> = Vec::new();
    for &n in fields {
        if is_html(dom, n, atom!("input")) && input_type(dom, n) == "radio" && checked(dom, n, c) {
            let name = attr(dom, n, atom!("name")).unwrap_or_default();
            match last.iter_mut().find(|(k, _)| *k == name) {
                Some(slot) => slot.1 = n,
                None => last.push((name, n)),
            }
        }
    }
    last.into_iter().map(|(_, n)| n).collect()
}

fn is_submittable(dom: &Dom, n: NodeId) -> bool {
    [atom!("button"), atom!("input"), atom!("select"), atom!("textarea")].into_iter().any(|a| is_html(dom, n, a))
}

/// The entry list of `form` submitted by `submitter`, with the controls'
/// current state from `c`.
pub fn entry_list(dom: &Dom, form: NodeId, submitter: Option<NodeId>, c: &dyn Controls) -> Vec<Entry> {
    let owner = Owner::new(dom, form);
    let fields: Vec<NodeId> =
        dom.descendants(root(dom, form)).filter(|&n| is_submittable(dom, n) && owner.owns(dom, n)).collect();
    let radios = radio_winners(dom, &fields, c);
    let mut out = Vec::new();
    for n in fields {
        let in_datalist = crate::node::ancestors(dom, n).any(|a| is_html(dom, a, atom!("datalist")));
        if in_datalist || is_disabled(dom, n) {
            continue;
        }
        field(dom, n, submitter, c, &radios, &mut out);
    }
    out
}

/// One entry per chosen file; with none, one empty file entry.
fn file_entries(name: &str, files: Vec<FileValue>, out: &mut Vec<Entry>) {
    if files.is_empty() {
        let empty = FileValue { filename: String::new(), content_type: "application/octet-stream".into(), path: None };
        out.push(Entry { name: name.to_owned(), value: Value::File(empty) });
    }
    out.extend(files.into_iter().map(|f| Entry { name: name.to_owned(), value: Value::File(f) }));
}

fn field(dom: &Dom, n: NodeId, submitter: Option<NodeId>, c: &dyn Controls, radios: &[NodeId], out: &mut Vec<Entry>) {
    let is_input = is_html(dom, n, atom!("input"));
    let ty = if is_input { input_type(dom, n) } else { "" };
    let is_button = is_html(dom, n, atom!("button")) || (is_input && matches!(ty, "submit" | "image" | "reset" | "button"));
    if is_button && Some(n) != submitter {
        return;
    }
    if is_input && ((ty == "checkbox" && !checked(dom, n, c)) || (ty == "radio" && !radios.contains(&n))) {
        return;
    }
    let name = attr(dom, n, atom!("name")).filter(|v| !v.is_empty());
    if ty == "image" {
        let (x, y) = match &name {
            Some(p) => (format!("{p}.x"), format!("{p}.y")),
            None => ("x".to_owned(), "y".to_owned()),
        };
        out.extend([text(&x, "0"), text(&y, "0")]);
        return;
    }
    let Some(name) = name else { return };
    if is_html(dom, n, atom!("select")) {
        for o in selected(dom, n, c) {
            if !option_disabled(dom, o) {
                out.push(text(&name, option_value(dom, o)));
            }
        }
    } else if matches!(ty, "checkbox" | "radio") {
        out.push(text(&name, attr(dom, n, atom!("value")).unwrap_or_else(|| "on".to_owned())));
    } else if ty == "file" {
        file_entries(&name, c.files(n).unwrap_or_default(), out);
    } else if ty == "hidden" && name.eq_ignore_ascii_case("_charset_") {
        out.push(text(&name, CHARSET));
    } else if is_html(dom, n, atom!("textarea")) {
        let raw = c.value(n).unwrap_or_else(|| text_content(dom, n));
        out.push(text(&name, raw.replace("\r\n", "\n").replace('\r', "\n")));
    } else {
        let raw = c.value(n).or_else(|| attr(dom, n, atom!("value"))).unwrap_or_default();
        let v = if is_input { sanitize(dom, n, ty, raw) } else { raw };
        out.push(text(&name, v));
    }
    let dirname_ok = is_html(dom, n, atom!("textarea")) || matches!(ty, "text" | "search" | "tel" | "url" | "email" | "password" | "hidden" | "submit");
    if let Some(d) = attr(dom, n, atom!("dirname")).filter(|d| !d.is_empty() && dirname_ok)
        && let Some(Entry { value: Value::Text(v), .. }) = out.last()
    {
        let dir = directionality(dom, n, v);
        out.push(text(&d, dir));
    }
}
