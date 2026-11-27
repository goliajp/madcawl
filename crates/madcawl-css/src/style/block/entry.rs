//! One declaration of a style attribute's block, its value serialized
//! when it is parsed: a value of one token from the token, which keeps
//! the keyword or unit the parsed value may not, a color keyword of a
//! shorthand by its name, and any other value from its parsed form. A
//! property the cascade does not compute keeps its value as written.

use crate::parser::{BlockItem, Declaration, parse_declaration_list};
use crate::tokenizer::{Kind, Tokenizer};

use super::super::props::{Decl, Prop, Specified, Weight, What};
use super::super::shorthand::Short;
use super::specified::write_specified;

/// What a declaration is of.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Key {
    Known(What),
    /// A property the cascade does not compute
    Other(&'static str),
}

/// One declaration of a block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub key: Key,
    /// The value serialized; empty for a longhand a shorthand holding
    /// `var()` set
    pub text: Box<str>,
    pub important: bool,
    /// For a longhand a shorthand holding `var()` set: the shorthand and
    /// its value as written
    pub pending: Option<(Short, Box<str>)>,
}

fn base(w: &What) -> Option<Prop> {
    match w {
        What::Prop(p) => Some(*p),
        What::Logical(l) => Some(l.base),
        What::Custom(_) => None,
    }
}

/// Whether an identifier value of `p` keeps its case: a name, not a
/// keyword.
fn names_something(p: Prop) -> bool {
    use Prop as P;
    matches!(
        p,
        P::FontFamily | P::GridRowStart | P::GridRowEnd | P::GridColumnStart | P::GridColumnEnd
    )
}

/// A value of one identifier, number with a unit or percentage written
/// from its token, which keeps the keyword or unit the parsed value may
/// not.
fn one_token(p: Prop, src: &str, d: &Declaration, out: &mut String) -> bool {
    let t = super::super::tokens(src, d.value);
    let [t] = &t[..] else { return false };
    match t.kind {
        Kind::Ident if !names_something(p) => {
            t.write_value(src, out);
            out.make_ascii_lowercase();
        }
        Kind::Dimension | Kind::Percentage => {
            super::super::cssom::number(t.number(src), out);
            if t.kind == Kind::Percentage {
                out.push('%');
            } else {
                out.extend(t.unit_chars(src).map(|c| c.to_ascii_lowercase()));
            }
        }
        _ => return false,
    }
    true
}

/// The keyword of a shorthand's value that gave a longhand the color or
/// the bold weight `v`, which keeps its name as a specified value.
fn keyword(src: &str, d: &Declaration, v: &Specified, out: &mut String) -> bool {
    let matches = |x: &super::super::vals::Item| match v {
        Specified::Color(c) => super::super::longhand::color(src, x).as_ref() == Some(c),
        Specified::Weight(Weight::Abs(700)) => x.tok.value_is(src, "bold"),
        _ => false,
    };
    let Some(xs) = super::super::vals::items(src, d.value, 64) else {
        return false;
    };
    let Some(x) = xs.iter().find(|x| x.kind() == Kind::Ident && matches(x)) else {
        return false;
    };
    x.tok.write_value(src, out);
    out.make_ascii_lowercase();
    true
}

fn trimmed(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_ascii_whitespace())
}

pub(crate) fn entry(src: &str, d: &Declaration, decl: Decl) -> Entry {
    let mut text = String::new();
    let mut pending = None;
    match (&decl.value, decl.short, base(&decl.what)) {
        (_, _, None) => text.push_str(decl.raw.as_deref().unwrap_or("")),
        (Specified::Unresolved, Some(sh), _) => {
            pending = Some((sh, trimmed(decl.raw.as_deref().unwrap_or("")).into()))
        }
        (Specified::Unresolved, None, _) => {
            text.push_str(trimmed(decl.raw.as_deref().unwrap_or("")))
        }
        (v @ Specified::Wide(_), _, Some(p)) => write_specified(p, v, &mut text),
        (v, sh, Some(p)) => {
            let kept = if sh.is_some() {
                keyword(src, d, v, &mut text)
            } else {
                one_token(p, src, d, &mut text)
            };
            if !kept {
                write_specified(p, v, &mut text);
            }
        }
    }
    Entry {
        key: Key::Known(decl.what),
        text: text.into(),
        important: decl.important,
        pending,
    }
}

/// Whether `text`, the value of a custom property (`blocks`) or of one
/// the cascade does not compute, is one value and nothing more once
/// written into a block: no `;` or `!` outside a block, no `{` or `}`
/// but in a custom property's balanced blocks, every bracket closed, and
/// nothing (a string, a comment) left open at its end, which would take
/// in the declarations written after it.
fn one_value(name: &str, text: &str, blocks: bool) -> bool {
    let mut t = Tokenizer::new(text);
    let mut open = Vec::new();
    loop {
        let k = t.next_token().kind;
        match k {
            Kind::Eof => break,
            Kind::BadString | Kind::BadUrl => return false,
            Kind::OpenCurly | Kind::CloseCurly if !blocks => return false,
            Kind::Semicolon | Kind::Delim(b'!') if open.is_empty() => return false,
            Kind::Function | Kind::OpenParen => open.push(Kind::CloseParen),
            Kind::OpenSquare => open.push(Kind::CloseSquare),
            Kind::OpenCurly => open.push(Kind::CloseCurly),
            Kind::CloseParen | Kind::CloseSquare | Kind::CloseCurly if open.pop() != Some(k) => {
                return false;
            }
            _ => {}
        }
    }
    // a string or comment the input ended inside reads past the value
    let probe = format!("{name}:{text};--end:0");
    let items = parse_declaration_list(&probe);
    open.is_empty()
        && matches!(&items[..], [BlockItem::Declaration(a), BlockItem::Declaration(b)]
            if !a.important && b.name.value_is(&probe, "--end"))
}

/// The text a declaration block keeps of such a value: the value, with a
/// comment it ends inside taken as closed there and left out, as the
/// end of the input closes it; `None` when it is no one value.
pub(crate) fn kept_value<'a>(name: &str, text: &'a str, blocks: bool) -> Option<&'a str> {
    if one_value(name, text, blocks) {
        return Some(text);
    }
    let i = text
        .rfind("/*")
        .filter(|&i| !text[i + 2..].contains("*/"))?;
    let t = trimmed(&text[..i]);
    (!t.is_empty() && one_value(name, t, blocks)).then_some(t)
}

/// A custom property's declaration kept as `kept_value` says.
pub(crate) fn checked_custom(mut e: Entry) -> Option<Entry> {
    let Key::Known(What::Custom(n)) = &e.key else {
        return Some(e);
    };
    let t = kept_value(n, &e.text, true)?;
    if t.len() != e.text.len() {
        e.text = t.into();
    }
    Some(e)
}

/// A declaration of a property the cascade does not compute: its value
/// as `kept_value` gives it, which is no value when it is empty.
pub(crate) fn other(name: &'static str, src: &str, d: &Declaration) -> Option<Entry> {
    let text = trimmed(d.value.of(src));
    let text = kept_value(name, text, false).filter(|t| !t.is_empty())?;
    Some(Entry {
        key: Key::Other(name),
        text: text.into(),
        important: d.important,
        pending: None,
    })
}
