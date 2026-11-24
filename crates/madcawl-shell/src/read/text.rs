//! Text as a reader writes it: collapsed white space, escapes, node
//! references, and the shown text of a subtree on one line.

use madcawl_dom::{NodeId, NodeKind};
use madcawl_html::Namespace;
use madcawl_str::{DomString, atom};

use super::Reader;

/// CSS's document white space.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// Calls `f` with the string as UTF-8; lone surrogates become U+FFFD.
pub(crate) fn with_str<R>(s: &DomString, f: impl FnOnce(&str) -> R) -> R {
    match s.as_ascii_str() {
        Some(a) => f(a),
        None => {
            let mut v = Vec::new();
            s.write_wtf8(&mut v);
            f(&String::from_utf8_lossy(&v))
        }
    }
}

pub(crate) fn string(s: &DomString) -> String {
    with_str(s, str::to_owned)
}

/// A line being written: runs of white space become one space, none at
/// the ends.
#[derive(Default)]
pub(crate) struct Line {
    pub s: String,
    space: bool,
    /// The line ends with a piece from `raw`
    token: bool,
}

impl Line {
    fn gap(&mut self) {
        if std::mem::take(&mut self.space) && !self.s.is_empty() {
            self.s.push(' ');
        }
    }

    pub fn text(&mut self, t: &str) {
        for c in t.chars() {
            if is_space(c) {
                self.space = true;
            } else {
                self.gap();
                self.s.push(c);
                self.token = false;
            }
        }
    }

    /// A piece that is already written, such as a link; two in a row
    /// are kept apart.
    pub fn raw(&mut self, t: &str) {
        self.space |= self.token;
        self.gap();
        self.s.push_str(t);
        self.token = true;
    }

    pub fn space(&mut self) {
        self.space = true;
    }
}

/// `e` and the node index in base 36: the same node gets the same
/// reference every time the same bytes are read.
pub fn reference(n: NodeId) -> String {
    let mut out = String::new();
    push_reference(&mut out, n);
    out
}

pub(crate) fn push_reference(out: &mut String, n: NodeId) {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut i = n.index();
    let mut buf = [0u8; 8];
    let mut k = buf.len();
    out.push('e');
    loop {
        k -= 1;
        buf[k] = DIGITS[(i % 36) as usize];
        i /= 36;
        if i == 0 {
            break;
        }
    }
    out.push_str(std::str::from_utf8(&buf[k..]).expect("ascii digits"));
}

/// The node index a reference names.
pub fn parse_reference(r: &str) -> Option<u32> {
    let digits = r.strip_prefix('e')?;
    (!digits.is_empty()).then_some(())?;
    u32::from_str_radix(digits, 36).ok()
}

/// A string in double quotes, its quotes, backslashes and line breaks
/// escaped.
pub(crate) fn quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `[name](href)[ref]`, with `]` in the name and `)` and spaces in the
/// href escaped.
pub(crate) fn link(name: &str, href: &str, n: NodeId) -> String {
    let mut out = String::from("[");
    for c in name.chars() {
        if matches!(c, ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push_str("](");
    for c in href.trim().chars() {
        match c {
            ' ' => out.push_str("%20"),
            ')' => out.push_str("%29"),
            c => out.push(c),
        }
    }
    out.push_str(")[");
    push_reference(&mut out, n);
    out.push(']');
    out
}

impl Reader<'_> {
    /// The shown text below `el` on one line, image `alt`s included,
    /// the contents of nested controls left out. Returns the level of
    /// the first heading in it.
    pub(crate) fn flat(&mut self, el: NodeId, line: &mut Line) -> Option<u8> {
        let mut heading = None;
        for c in self.dom.children(el) {
            match self.dom.kind(c) {
                NodeKind::Text if self.shown.shown(c) => {
                    if let Some(t) = self.dom.text(c) {
                        with_str(t, |s| line.text(s));
                        self.used.push(c);
                    }
                }
                NodeKind::Element if self.shown.rendered(c) => {
                    let name = self.dom.local_name(c);
                    let html = self.dom.namespace(c) == Some(Namespace::Html);
                    if html
                        && matches!(name, Some(n) if n == atom!("select") || n == atom!("textarea"))
                    {
                        continue;
                    }
                    if html && name == Some(atom!("br")) {
                        line.space();
                        continue;
                    }
                    if html && name == Some(atom!("img")) && self.shown.shown(c) {
                        if let Some(alt) = self.dom.attribute(c, atom!("alt")) {
                            with_str(alt, |s| line.text(s));
                            self.used.push(c);
                        }
                        continue;
                    }
                    // a block boundary separates words
                    let block = self.block_level(c);
                    if block {
                        line.space();
                    }
                    let level = heading_level(name).filter(|_| html);
                    let inner = self.flat(c, line);
                    heading = heading.or(level).or(inner);
                    if block {
                        line.space();
                    }
                }
                _ => {}
            }
        }
        heading
    }

    /// The attribute, trimmed, when it is not empty.
    pub(crate) fn attr(&self, el: NodeId, name: madcawl_str::Atom) -> Option<String> {
        let v = self.dom.attribute(el, name)?;
        let s = with_str(v, |s| s.trim().to_owned());
        (!s.is_empty()).then_some(s)
    }
}

pub(crate) fn heading_level(name: Option<madcawl_str::Atom>) -> Option<u8> {
    let n = name?;
    [
        atom!("h1"),
        atom!("h2"),
        atom!("h3"),
        atom!("h4"),
        atom!("h5"),
        atom!("h6"),
    ]
    .iter()
    .position(|&h| h == n)
    .map(|i| i as u8 + 1)
}
