//! Selectors back to text, as CSSOM serializes them: identifiers and
//! strings escaped so that parsing the text gives the same selector.

use std::fmt::Write;

use super::{
    AttrOp, CaseFlag, Combinator, Component, Dir, Namespace, Nth, NthKind, Selector, SelectorList,
};
use crate::names::{Name, Names};

fn hex_escape(c: char, out: &mut String) {
    let _ = write!(out, "\\{:x} ", c as u32);
}

/// CSSOM "serialize an identifier".
pub fn write_ident(s: &str, out: &mut String) {
    let first_dash = s.starts_with('-');
    if s == "-" {
        out.push_str("\\-");
        return;
    }
    for (i, c) in s.chars().enumerate() {
        match c {
            '\0' => out.push('\u{FFFD}'),
            '\u{1}'..='\u{1f}' | '\u{7f}' => hex_escape(c, out),
            '0'..='9' if i == 0 || (i == 1 && first_dash) => hex_escape(c, out),
            '-' | '_' | 'a'..='z' | 'A'..='Z' | '0'..='9' => out.push(c),
            _ if c as u32 >= 0x80 => out.push(c),
            _ => {
                out.push('\\');
                out.push(c);
            }
        }
    }
}

/// CSSOM "serialize a string".
pub fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\0' => out.push('\u{FFFD}'),
            '\u{1}'..='\u{1f}' | '\u{7f}' => hex_escape(c, out),
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
}

struct W<'a> {
    names: &'a Names,
    out: &'a mut String,
}

impl W<'_> {
    fn ident(&mut self, n: Name) {
        write_ident(self.names.get(n), self.out);
    }

    fn ns(&mut self, ns: Namespace) {
        match ns {
            Namespace::Implicit(_) => {}
            Namespace::Any => self.out.push_str("*|"),
            Namespace::None => self.out.push('|'),
            Namespace::Prefix { prefix, .. } => {
                self.ident(prefix);
                self.out.push('|');
            }
        }
    }

    fn list(&mut self, l: &SelectorList) {
        for (i, s) in l.0.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.selector(s);
        }
    }

    fn selector(&mut self, s: &Selector) {
        let mut comps: &[Component] = &s.components;
        if let Some(Component::Combinator(c)) = comps.last() {
            // a relative selector's combinator to its anchor
            comps = &comps[..comps.len() - 1];
            if *c != Combinator::Descendant {
                self.combinator(*c);
                self.out.push(' ');
            }
        }
        let parts: Vec<&[Component]> = comps
            .split(|c| matches!(c, Component::Combinator(_)))
            .collect();
        let combs: Vec<Combinator> = comps
            .iter()
            .filter_map(|c| match c {
                Component::Combinator(c) => Some(*c),
                _ => None,
            })
            .collect();
        for (i, part) in parts.iter().enumerate().rev() {
            for c in part.iter() {
                self.simple(c);
            }
            if i > 0 {
                match combs[i - 1] {
                    Combinator::Descendant => self.out.push(' '),
                    c => {
                        self.out.push(' ');
                        self.combinator(c);
                        self.out.push(' ');
                    }
                }
            }
        }
    }

    fn combinator(&mut self, c: Combinator) {
        self.out.push_str(match c {
            Combinator::Descendant => "",
            Combinator::Child => ">",
            Combinator::NextSibling => "+",
            Combinator::SubsequentSibling => "~",
        });
    }

    fn func(&mut self, name: &str, l: &SelectorList) {
        self.out.push(':');
        self.out.push_str(name);
        self.out.push('(');
        self.list(l);
        self.out.push(')');
    }

    fn simple(&mut self, c: &Component) {
        match c {
            Component::Combinator(_) => {}
            Component::Universal(ns) => {
                self.ns(*ns);
                self.out.push('*');
            }
            Component::Type { ns, name, .. } => {
                self.ns(*ns);
                self.ident(*name);
            }
            Component::Id(n) => {
                self.out.push('#');
                self.ident(*n);
            }
            Component::Class(n) => {
                self.out.push('.');
                self.ident(*n);
            }
            Component::Attribute(a) => {
                self.out.push('[');
                self.ns(a.ns);
                self.ident(a.name);
                if let Some((op, value, case)) = a.op {
                    self.out.push_str(match op {
                        AttrOp::Equals => "=",
                        AttrOp::Includes => "~=",
                        AttrOp::DashMatch => "|=",
                        AttrOp::Prefix => "^=",
                        AttrOp::Suffix => "$=",
                        AttrOp::Substring => "*=",
                    });
                    write_string(self.names.get(value), self.out);
                    self.out.push_str(match case {
                        CaseFlag::Default => "",
                        CaseFlag::Insensitive => " i",
                        CaseFlag::Sensitive => " s",
                    });
                }
                self.out.push(']');
            }
            Component::PseudoClass(p) => {
                self.out.push(':');
                self.out.push_str(p.name());
            }
            Component::Nth(n) => self.nth(n),
            Component::Not(l) => self.func("not", l),
            Component::Is(l) => self.func("is", l),
            Component::Where(l) => self.func("where", l),
            Component::Has(l) => self.func("has", l),
            Component::Lang(langs) => {
                self.out.push_str(":lang(");
                for (i, l) in langs.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    write_string(self.names.get(*l), self.out);
                }
                self.out.push(')');
            }
            Component::Dir(d) => self.out.push_str(match d {
                Dir::Ltr => ":dir(ltr)",
                Dir::Rtl => ":dir(rtl)",
            }),
            Component::PseudoElement(e) => {
                self.out.push_str("::");
                self.out.push_str(e.name());
            }
            Component::Nesting => self.out.push('&'),
        }
    }

    fn nth(&mut self, n: &Nth) {
        self.out.push_str(match n.kind {
            NthKind::Child => ":nth-child(",
            NthKind::LastChild => ":nth-last-child(",
            NthKind::OfType => ":nth-of-type(",
            NthKind::LastOfType => ":nth-last-of-type(",
        });
        let _ = match (n.a, n.b) {
            (0, b) => write!(self.out, "{b}"),
            (a, b) => {
                let _ = match a {
                    1 => write!(self.out, "n"),
                    -1 => write!(self.out, "-n"),
                    a => write!(self.out, "{a}n"),
                };
                match b {
                    0 => Ok(()),
                    b if b > 0 => write!(self.out, "+{b}"),
                    b => write!(self.out, "-{}", -(b as i64)),
                }
            }
        };
        if let Some(of) = &n.of {
            self.out.push_str(" of ");
            self.list(of);
        }
        self.out.push(')');
    }
}

impl SelectorList {
    /// The list as CSSOM text, appended to `out`.
    pub fn to_css(&self, names: &Names, out: &mut String) {
        W { names, out }.list(self);
    }
}
