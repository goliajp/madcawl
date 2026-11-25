//! Strings out of the DOM, and the whitespace rule names and views use.

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::{Atom, DomString};

/// A DOM string as UTF-8; lone surrogates become U+FFFD.
pub(crate) fn string(s: &DomString) -> String {
    if let Some(a) = s.as_ascii_str() {
        return a.to_owned();
    }
    let mut b = Vec::with_capacity(s.len());
    s.write_wtf8(&mut b);
    String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

pub(crate) fn attr_string(dom: &Dom, n: NodeId, a: Atom) -> Option<String> {
    if dom.kind(n) != NodeKind::Element {
        return None;
    }
    dom.attribute(n, a).map(string)
}

/// The attribute is there and not only whitespace.
pub(crate) fn attr_filled(dom: &Dom, n: NodeId, a: Atom) -> Option<String> {
    attr_string(dom, n, a).filter(|v| !is_blank(v))
}

pub(crate) fn is_blank(s: &str) -> bool {
    s.bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c'))
}

/// Appends `s` to `out` with runs of whitespace as one space, never a
/// space at the start of `out` or two in a row.
pub(crate) fn push_collapsed(out: &mut String, s: &str) {
    for c in s.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c') {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
}

/// `s` with whitespace collapsed and trimmed.
pub(crate) fn collapsed(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    push_collapsed(&mut out, s);
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// The tokens of an ID reference list such as `aria-labelledby`.
pub(crate) fn id_list(v: &str) -> impl Iterator<Item = &str> {
    v.split(|c: char| c.is_ascii_whitespace())
        .filter(|t| !t.is_empty())
}

/// The text content of `root`: the data of every text node below it,
/// in tree order.
pub(crate) fn text_content(dom: &Dom, root: NodeId) -> String {
    let mut out = String::new();
    for n in dom.descendants(root) {
        if dom.kind(n) == NodeKind::Text
            && let Some(t) = dom.text(n)
        {
            out.push_str(&string(t));
        }
    }
    out
}

/// The estimated tokens of a text: an ASCII byte is a quarter of one,
/// any other character one (no model's tokenizer is used).
pub fn estimate_tokens(s: &str) -> usize {
    let mut ascii = 0usize;
    let mut other = 0usize;
    for c in s.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    ascii.div_ceil(4) + other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_collapses_and_tokens_are_estimated() {
        assert_eq!(collapsed("  a \n\t b  "), "a b");
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens("中文ab"), 3);
        assert_eq!(id_list(" a  b ").collect::<Vec<_>>(), ["a", "b"]);
    }
}
