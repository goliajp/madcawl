//! Presentational hints: the attributes HTML and SVG map to CSS
//! (HTML, "Rendering"; SVG 2, "presentation attributes"), written out
//! as a declaration block. The cascade puts it before every author
//! rule, at specificity zero.

use std::fmt::Write;

use madcawl_dom::{Dom, NodeId};
use madcawl_html::Namespace;
use madcawl_str::{Atom, atom};

use super::matching::text;

/// A non-negative integer or percentage as HTML's dimension values
/// parse it, written as a CSS length; `None` for zero when `zero` is
/// not allowed, and for what does not parse.
fn dimension(v: &str, zero: bool) -> Option<String> {
    let v = v.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
    let digits = v.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &v[digits..];
    let frac = rest
        .strip_prefix('.')
        .map_or(0, |r| 1 + r.bytes().take_while(u8::is_ascii_digit).count());
    let n: f64 = v[..digits + frac].trim_end_matches('.').parse().ok()?;
    if n == 0.0 && !zero {
        return None;
    }
    let pct = v[digits + frac..].starts_with('%');
    Some(format!("{n}{}", if pct { "%" } else { "px" }))
}

fn push(out: &mut String, prop: &str, value: &str) {
    let _ = write!(out, "{prop}: {value}; ");
}

/// The properties an SVG presentation attribute of the same name sets,
/// among those the cascade computes.
const SVG_PRESENTATION: &[&str] = &[
    "color",
    "direction",
    "display",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "opacity",
    "overflow",
    "visibility",
    "writing-mode",
];

fn svg(dom: &Dom, el: NodeId, name: Atom, out: &mut String) {
    let sized = [
        atom!("svg"),
        atom!("rect"),
        atom!("image"),
        atom!("foreignObject"),
    ];
    for a in dom.attributes(el) {
        if a.ns != madcawl_html::AttrNamespace::None {
            continue;
        }
        let local = a.local.to_dom_string();
        let local = text(&local);
        let v = text(a.value);
        if (local == "width" || local == "height") && sized.contains(&name) {
            // a length, with or without a unit; `auto` and garbage are ignored
            let v = v.trim();
            if v.parse::<f64>().is_ok() {
                push(out, &local, &format!("{v}px"));
            } else if !v.is_empty() && !v.contains([';', '{', '}', '!']) {
                push(out, &local, v);
            }
        } else if SVG_PRESENTATION.contains(&&*local) && !v.contains([';', '{', '}', '!']) {
            push(out, &local, &v);
        }
    }
}

fn align(v: &str) -> Option<&'static str> {
    Some(match v.trim().to_ascii_lowercase().as_str() {
        "left" => "-webkit-left",
        "right" => "-webkit-right",
        "center" | "middle" => "-webkit-center",
        "justify" => "justify",
        _ => return None,
    })
}

/// The hints of `el` as a declaration block, or `None` when it has none.
pub(crate) fn hints(dom: &Dom, el: NodeId) -> Option<String> {
    let name = dom.local_name(el)?;
    let mut out = String::new();
    match dom.namespace(el)? {
        Namespace::Svg => svg(dom, el, name, &mut out),
        Namespace::Html => html(dom, el, name, &mut out),
        _ => {}
    }
    (!out.is_empty()).then_some(out)
}

fn html(dom: &Dom, el: NodeId, name: Atom, out: &mut String) {
    let get = |a: Atom| dom.attribute(el, a).map(text);
    let is = |names: &[Atom]| names.contains(&name);
    let sized = is(&[
        atom!("img"),
        atom!("iframe"),
        atom!("embed"),
        atom!("object"),
        atom!("video"),
        atom!("canvas"),
        atom!("table"),
        atom!("td"),
        atom!("th"),
        atom!("col"),
        atom!("hr"),
    ]);
    if sized || is(&[atom!("tr")]) {
        for (a, prop) in [(atom!("width"), "width"), (atom!("height"), "height")] {
            let applies = match prop {
                "width" => !is(&[atom!("tr")]),
                _ => !is(&[atom!("hr"), atom!("col")]),
            };
            if applies && let Some(v) = get(a).and_then(|v| dimension(&v, !is(&[atom!("table")]))) {
                push(out, prop, &v);
            }
        }
    }
    let cells = is(&[
        atom!("td"),
        atom!("th"),
        atom!("tr"),
        atom!("thead"),
        atom!("tbody"),
        atom!("tfoot"),
    ]);
    if (cells
        || is(&[
            atom!("div"),
            atom!("p"),
            atom!("h1"),
            atom!("h2"),
            atom!("h3"),
            atom!("h4"),
            atom!("h5"),
            atom!("h6"),
            atom!("caption"),
            atom!("legend"),
        ]))
        && let Some(v) = get(atom!("align")).as_deref().and_then(align)
    {
        push(out, "text-align", v);
    }
    if cells {
        if let Some(v) = get(atom!("valign")) {
            let v = v.trim().to_ascii_lowercase();
            if matches!(v.as_str(), "top" | "middle" | "bottom" | "baseline") {
                push(out, "vertical-align", &v);
            }
        }
        if is(&[atom!("td"), atom!("th")]) && get(atom!("nowrap")).is_some() {
            push(out, "white-space", "nowrap");
        }
    }
    if is(&[
        atom!("body"),
        atom!("table"),
        atom!("td"),
        atom!("th"),
        atom!("tr"),
        atom!("thead"),
        atom!("tbody"),
        atom!("tfoot"),
    ]) && let Some(v) = get(atom!("bgcolor"))
        && !v.contains([';', '{', '}', '!'])
    {
        push(out, "background-color", v.trim());
    }
    if is(&[atom!("table")])
        && let Some(v) = get(atom!("align"))
    {
        match v.trim().to_ascii_lowercase().as_str() {
            "left" => push(out, "float", "left"),
            "right" => push(out, "float", "right"),
            "center" => push(out, "margin-inline", "auto"),
            _ => {}
        }
    }
    if is(&[atom!("body")])
        && let Some(v) = get(atom!("text"))
        && !v.contains([';', '{', '}', '!'])
    {
        push(out, "color", v.trim());
    }
    if is(&[atom!("font")]) {
        font(&get, out);
    }
}

fn font<'d>(get: &dyn Fn(Atom) -> Option<std::borrow::Cow<'d, str>>, out: &mut String) {
    if let Some(v) = get(atom!("color"))
        && !v.contains([';', '{', '}', '!'])
    {
        push(out, "color", v.trim());
    }
    if let Some(v) = get(atom!("face"))
        && !v.contains([';', '{', '}', '!', '"', '\\'])
    {
        push(out, "font-family", v.trim());
    }
    if let Some(v) = get(atom!("size")) {
        // the legacy font size of html: 1 to 7, or relative to 3
        let v = v.trim();
        let (sign, digits) = match v.as_bytes().first() {
            Some(b'+') => (1, &v[1..]),
            Some(b'-') => (-1, &v[1..]),
            _ => (0, v),
        };
        let n: String = digits.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = n.parse::<i32>() {
            let k = if sign == 0 { n } else { 3 + sign * n }.clamp(1, 7);
            let kw = [
                "x-small",
                "small",
                "medium",
                "large",
                "x-large",
                "xx-large",
                "xxx-large",
            ];
            push(out, "font-size", kw[(k - 1) as usize]);
        }
    }
}
