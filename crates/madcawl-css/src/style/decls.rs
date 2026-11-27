//! A declaration block's declarations, parsed: the longhands each one
//! sets, shorthands and `all` expanded.

use crate::Span;
use crate::parser::Declaration;

use super::logical::{Logical, logical_name};
use super::props::{Prop, Specified, has_var, parse, wide};
use super::shorthand::{self, Short};
use super::vals::items;
use super::var;

/// What a declaration sets.
#[derive(Clone, Debug, PartialEq)]
pub enum What {
    Prop(Prop),
    /// A flow-relative property, which side it sets known per element
    Logical(Logical),
    /// A custom property, by its name with the dashes
    Custom(Box<str>),
}

/// One parsed declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    pub what: What,
    /// For a custom property, a CSS-wide keyword or `Unresolved`
    pub value: Specified,
    /// The value as written, when `var()` has to be substituted into it,
    /// and a custom property's value, whitespace trimmed
    pub raw: Option<Box<str>>,
    /// The shorthand `raw` is the value of, when it is one
    pub short: Option<Short>,
    pub important: bool,
    /// A custom property's value has `var()` or `env()` in it
    pub substitutes: bool,
}

/// Longhands and shorthands other engines name with a prefix or an
/// older name, by the name this engine knows them by.
pub(crate) fn alias(name: &str) -> &str {
    super::known_names::ALIASES
        .iter()
        .find(|a| a.0 == name)
        .map_or(name, |a| a.1)
}

/// What a longhand name sets.
pub(crate) fn target(name: &str) -> Option<What> {
    Prop::from_name(name)
        .map(What::Prop)
        .or_else(|| logical_name(name).map(What::Logical))
}

/// The declarations of a block this engine computes, in order; `all`
/// with a CSS-wide keyword stands for each property but the custom
/// ones, and a shorthand for its longhands.
pub fn parse_block(src: &str, decls: &[Declaration]) -> Box<[Decl]> {
    let mut out = Vec::new();
    let mut name = String::new();
    for d in decls {
        name.clear();
        d.name.write_value(src, &mut name);
        let text = d.value.of(src);
        if name.starts_with("--") && name != "--" {
            if !var::vars_valid(src, d.value) {
                continue;
            }
            let raw = text.trim_matches(|c: char| c.is_ascii_whitespace());
            out.push(Decl {
                what: What::Custom(name.as_str().into()),
                value: wide(src, d.value).map_or(Specified::Unresolved, Specified::Wide),
                raw: Some(raw.into()),
                short: None,
                important: d.important,
                substitutes: var::has_var(raw),
            });
            continue;
        }
        name.make_ascii_lowercase();
        let name = alias(&name);
        let decl = |what: What, value: Specified, short: Option<Short>| Decl {
            raw: (value == Specified::Unresolved).then(|| text.into()),
            what,
            value,
            short,
            important: d.important,
            substitutes: false,
        };
        if name == "all" {
            if let Some(w) = wide(src, d.value) {
                out.extend(
                    Prop::ALL
                        .iter()
                        .map(|&p| decl(What::Prop(p), Specified::Wide(w), None)),
                );
            }
            continue;
        }
        if let Some(what) = target(name) {
            let p = match &what {
                What::Prop(p) => *p,
                What::Logical(l) => l.base,
                What::Custom(_) => unreachable!("not a longhand"),
            };
            if let Some(v) = parse(p, src, d.value) {
                out.push(decl(what, v, None));
            }
            continue;
        }
        let Some(sh) = shorthand::by_name(name) else {
            continue;
        };
        let values = if has_var(src, d.value) {
            if !var::vars_valid(src, d.value) {
                continue;
            }
            shorthand::longhands(sh)
                .iter()
                .map(|w| (w.clone(), Specified::Unresolved))
                .collect()
        } else if let Some(w) = wide(src, d.value) {
            shorthand::longhands(sh)
                .iter()
                .map(|t| (t.clone(), Specified::Wide(w)))
                .collect()
        } else {
            match items(src, d.value, 64).and_then(|xs| shorthand::parse(sh, src, &xs)) {
                Some(v) => v,
                None => continue,
            }
        };
        out.extend(values.into_iter().map(|(w, v)| decl(w, v, Some(sh))));
    }
    out.into()
}

/// Whether `name: src[value]` is a declaration this engine computes;
/// `None` when it does not know the property.
pub(crate) fn declaration_parses(name: &str, src: &str, value: Span) -> Option<bool> {
    let name = alias(name);
    if let Some(what) = target(name) {
        let p = match what {
            What::Prop(p) => p,
            What::Logical(l) => l.base,
            What::Custom(_) => return Some(true),
        };
        return Some(parse(p, src, value).is_some());
    }
    let sh = shorthand::by_name(name)?;
    Some(
        has_var(src, value) && var::vars_valid(src, value)
            || wide(src, value).is_some()
            || items(src, value, 64)
                .and_then(|xs| shorthand::parse(sh, src, &xs))
                .is_some(),
    )
}
