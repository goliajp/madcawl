//! A CSS declaration block as CSSOM keeps one for a style attribute: its
//! declarations in order, each a longhand, a custom property or a
//! property the cascade does not compute, with its value already
//! serialized, read and changed by property name, and the whole block
//! written back out as `cssText` is. A shorthand sets and removes its
//! longhands, and reads back as the shorthand when every longhand it
//! sets is there.

mod css_text;
mod entry;
mod short;
mod specified;

use crate::parser::{BlockItem, Declaration, parse_declaration, parse_declaration_list};

use super::decls::target;
use super::known_names::{ShortInfo, canonical, logical_css_name, short, shorts, uncomputed};
use super::props::{What, parse_block};
use super::shorthand::Short;
use entry::{Entry, Key, checked_custom, entry, other};
use short::{MAX_LONGHANDS, write_short};

/// What a property name names.
enum Named<'a> {
    Custom(&'a str),
    Long(What),
    Short(Short),
    Other(&'static str),
}

/// A name as CSSOM takes it: a custom property's as it is, any other
/// ASCII-lowercase. `None` for a property this engine does not know.
fn named(name: &str) -> Option<Named<'_>> {
    if name.starts_with("--") {
        return Some(Named::Custom(name));
    }
    let c = canonical(name)?;
    if let Some(o) = uncomputed(c) {
        return Some(Named::Other(o));
    }
    Some(match target(c) {
        Some(w) => Named::Long(w),
        None => Named::Short(short(c).expect("a known name is a longhand or a shorthand")),
    })
}

fn same(k: &Key, n: &Named<'_>) -> bool {
    match (k, n) {
        (Key::Known(What::Custom(a)), Named::Custom(b)) => &**a == *b,
        (Key::Known(w), Named::Long(l)) => w == l,
        (Key::Other(a), Named::Other(b)) => a == b,
        _ => false,
    }
}

/// The places of a shorthand's longhands in a block, in its order.
type Parts = ([u16; MAX_LONGHANDS], usize);

/// A style attribute's declarations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declarations {
    entries: Vec<Entry>,
}

impl Declarations {
    pub fn new() -> Declarations {
        Declarations::default()
    }

    /// The declarations of a style attribute's value: those this engine
    /// knows, each a later one of a property taking the place of an
    /// earlier one, unless only the earlier one is important.
    pub fn parse(text: &str) -> Declarations {
        let mut out = Declarations::new();
        let mut name = String::new();
        for item in parse_declaration_list(text) {
            let BlockItem::Declaration(d) = item else {
                continue;
            };
            name.clear();
            d.name.write_value(text, &mut name);
            name.make_ascii_lowercase();
            if let Some(o) = uncomputed(super::decls::alias(&name)) {
                if let Some(e) = other(o, text, &d) {
                    out.push_parsed(e);
                }
                continue;
            }
            for decl in parse_block(text, std::slice::from_ref(&d)) {
                if let Some(e) = checked_custom(entry(text, &d, decl)) {
                    out.push_parsed(e);
                }
            }
        }
        out
    }

    /// `e` after the declarations parsed before it: the earlier one of
    /// its property goes, unless only that one is important.
    fn push_parsed(&mut self, e: Entry) {
        match self.entries.iter().position(|x| x.key == e.key) {
            Some(i) if self.entries[i].important && !e.important => return,
            Some(i) => drop(self.entries.remove(i)),
            None => {}
        }
        self.entries.push(e);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The property name of declaration `i`.
    pub fn name(&self, i: usize) -> Option<&str> {
        Some(key_name(&self.entries.get(i)?.key))
    }

    fn find(&self, n: &Named<'_>) -> Option<&Entry> {
        self.entries.iter().find(|e| same(&e.key, n))
    }

    /// The places of the longhands of `s`, when the block has them all.
    fn parts(&self, s: &ShortInfo) -> Option<Parts> {
        let mut at = [0u16; MAX_LONGHANDS];
        for (k, w) in s.longs.iter().enumerate() {
            let i = self
                .entries
                .iter()
                .position(|e| matches!(&e.key, Key::Known(x) if x == w))?;
            at[k] = i as u16;
        }
        Some((at, s.longs.len()))
    }

    /// The shorthand `s` written from the longhands at `p`, appended.
    fn write_parts(&self, s: &ShortInfo, p: &Parts, out: &mut String) {
        let refs: [&Entry; MAX_LONGHANDS] =
            std::array::from_fn(|k| &self.entries[usize::from(p.0[k.min(p.1 - 1)])]);
        write_short(s.sh, &refs[..p.1], out);
    }

    /// `getPropertyValue`: the value of the property `name` (see
    /// `named`) appended to `out`; nothing when the block does not have
    /// it, or not every longhand of a shorthand, or those with
    /// different priorities.
    pub fn value(&self, name: &str, out: &mut String) {
        match named(name) {
            Some(Named::Short(sh)) => {
                let s = shorts().find(sh);
                if let Some(p) = self.parts(s)
                    && self.one_priority(&p).is_some()
                {
                    self.write_parts(s, &p, out);
                }
            }
            Some(n) => {
                if let Some(e) = self.find(&n) {
                    out.push_str(&e.text);
                }
            }
            None => {}
        }
    }

    /// The priority of the longhands at `p`, when they have one.
    fn one_priority(&self, p: &Parts) -> Option<bool> {
        let first = self.entries[usize::from(p.0[0])].important;
        p.0[..p.1]
            .iter()
            .all(|&i| self.entries[usize::from(i)].important == first)
            .then_some(first)
    }

    /// `getPropertyPriority`: whether the property is important; a
    /// shorthand when each of its longhands is.
    pub fn important(&self, name: &str) -> bool {
        match named(name) {
            Some(Named::Short(sh)) => self
                .parts(shorts().find(sh))
                .is_some_and(|p| self.one_priority(&p) == Some(true)),
            Some(n) => self.find(&n).is_some_and(|e| e.important),
            None => false,
        }
    }

    /// `setProperty` with a value that is not empty: the property's
    /// declarations from `value`, each in the place of the one it
    /// replaces, or last. `false`, with nothing changed, for a property
    /// this engine does not know, a value that is not one of it, or the
    /// same declarations as before. The value of a property the cascade
    /// does not compute is taken as it is written.
    pub fn set(&mut self, name: &str, value: &str, important: bool) -> bool {
        let Some(n) = named(name) else { return false };
        let key = match &n {
            Named::Custom(c) => c,
            _ => canonical(name).expect("a known name"),
        };
        let src = format!("{key}:{value}");
        let Ok(d) = parse_declaration(&src) else {
            return false;
        };
        // `!important` is no part of a value
        if d.important {
            return false;
        }
        let Named::Other(o) = n else {
            let mut changed = false;
            for mut decl in parse_block(&src, std::slice::from_ref(&d)) {
                decl.important = important;
                let Some(e) = checked_custom(entry(&src, &d, decl)) else {
                    return false;
                };
                changed |= self.put(e);
            }
            return changed;
        };
        let d = Declaration { important, ..d };
        other(o, &src, &d).is_some_and(|e| self.put(e))
    }

    /// `e` in the place of the declaration of its property, or last;
    /// whether that changed the block.
    fn put(&mut self, e: Entry) -> bool {
        match self.entries.iter().position(|x| x.key == e.key) {
            Some(i) if self.entries[i] == e => false,
            Some(i) => {
                self.entries[i] = e;
                true
            }
            None => {
                self.entries.push(e);
                true
            }
        }
    }

    /// `removeProperty`'s removal: whether the block had a declaration
    /// of the property, or of one of the shorthand's longhands.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.entries.len();
        match named(name) {
            Some(Named::Short(sh)) => {
                let ws = &shorts().find(sh).longs;
                self.entries
                    .retain(|e| !matches!(&e.key, Key::Known(w) if ws.contains(w)));
            }
            Some(n) => self.entries.retain(|e| !same(&e.key, &n)),
            None => {}
        }
        self.entries.len() != before
    }
}

fn key_name(k: &Key) -> &str {
    match k {
        Key::Known(What::Prop(p)) => p.name(),
        Key::Known(What::Logical(l)) => logical_css_name(*l),
        Key::Known(What::Custom(c)) => c,
        Key::Other(n) => n,
    }
}
