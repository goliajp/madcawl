//! Custom properties and `var()`.
//!
//! An element's custom properties are a set that never changes once
//! made. An element that declares none shares its parent's set; one
//! that declares some gets a new set of only those, over its parent's.
//! A chain deeper than `MAX_DEPTH` is flattened into one set, so a
//! lookup walks a bounded number of sets.

use crate::Span;
use crate::tokenizer::{Kind, Tokenizer};

use super::props::{Decl, Specified, Wide};
use super::rules::NONE;

const MAX_DEPTH: u8 = 8;

/// The longest text substitution may make; longer is guaranteed-invalid,
/// as a few properties that each use the last twice would otherwise
/// grow exponentially.
const MAX_LEN: usize = 1 << 20;

/// A name and its value, `None` for the guaranteed-invalid value.
type Var = (Box<str>, Option<Box<str>>);

/// A set of custom properties: its own, sorted by name, over its parent's.
/// A name whose value is `None` is the guaranteed-invalid value: it
/// hides the parent's.
pub(crate) struct CustomSet {
    parent: u32,
    depth: u8,
    vars: Box<[Var]>,
}

/// The value of `name` in set `k` (`NONE` for the empty set).
pub(crate) fn lookup<'s>(sets: &'s [CustomSet], mut k: u32, name: &str) -> Option<&'s str> {
    while k != NONE {
        let s = &sets[k as usize];
        if let Ok(i) = s.vars.binary_search_by(|(n, _)| (**n).cmp(name)) {
            return s.vars[i].1.as_deref();
        }
        k = s.parent;
    }
    None
}

/// Every name and value of set `k`, the nearest set's winning.
pub(crate) fn all(sets: &[CustomSet], mut k: u32) -> Vec<(&str, &str)> {
    let mut seen: Vec<(&str, Option<&str>)> = Vec::new();
    while k != NONE {
        let s = &sets[k as usize];
        for (n, v) in s.vars.iter() {
            if !seen.iter().any(|(m, _)| *m == &**n) {
                seen.push((n, v.as_deref()));
            }
        }
        k = s.parent;
    }
    let mut out: Vec<(&str, &str)> = seen
        .into_iter()
        .filter_map(|(n, v)| v.map(|v| (n, v)))
        .collect();
    out.sort_unstable();
    out
}

enum State {
    Todo,
    Busy,
    Done(Option<String>),
}

/// An element's declared custom properties resolved over its parent's
/// set: `var()` in them substituted, a cycle making every property in
/// it guaranteed-invalid, fallbacks or not.
struct Resolver<'d, 's> {
    declared: &'d [(&'d str, &'d Decl)],
    state: Vec<State>,
    /// The properties being resolved, outermost first
    stack: Vec<usize>,
    cyclic: Vec<bool>,
    sets: &'s [CustomSet],
    parent: u32,
}

impl Resolver<'_, '_> {
    fn get(&mut self, name: &str) -> Option<String> {
        match self.declared.iter().position(|(n, _)| *n == name) {
            Some(i) => self.value(i),
            None => lookup(self.sets, self.parent, name).map(str::to_owned),
        }
    }

    fn value(&mut self, i: usize) -> Option<String> {
        match &self.state[i] {
            State::Done(v) => return v.clone(),
            State::Busy => {
                let at = self
                    .stack
                    .iter()
                    .position(|&j| j == i)
                    .expect("busy is stacked");
                for &j in &self.stack[at..] {
                    self.cyclic[j] = true;
                }
                return None;
            }
            State::Todo => {}
        }
        self.state[i] = State::Busy;
        self.stack.push(i);
        let (name, d) = self.declared[i];
        let v = match (&d.value, &d.raw) {
            (Specified::Wide(Wide::Initial), _) => None,
            (Specified::Wide(_), _) => lookup(self.sets, self.parent, name).map(str::to_owned),
            (_, Some(raw)) if has_var(raw) => substitute(raw, false, &mut |n| self.get(n)),
            (_, raw) => Some(raw.as_deref().unwrap_or_default().to_owned()),
        };
        self.stack.pop();
        let v = v.filter(|_| !self.cyclic[i]);
        self.state[i] = State::Done(v.clone());
        v
    }
}

/// The set an element gets: its parent's when it declares none.
pub(crate) fn element_set(
    sets: &mut Vec<CustomSet>,
    parent: u32,
    declared: &[(&str, &Decl)],
) -> u32 {
    if declared.is_empty() {
        return parent;
    }
    let mut r = Resolver {
        declared,
        state: declared.iter().map(|_| State::Todo).collect(),
        stack: Vec::new(),
        cyclic: vec![false; declared.len()],
        sets,
        parent,
    };
    let mut vars: Vec<Var> = (0..declared.len())
        .map(|i| (declared[i].0.into(), r.value(i).map(Into::into)))
        .collect();
    vars.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let depth = if parent == NONE {
        0
    } else {
        sets[parent as usize].depth + 1
    };
    let set = if depth < MAX_DEPTH {
        CustomSet {
            parent,
            depth,
            vars: vars.into(),
        }
    } else {
        let mut flat: Vec<Var> = all(sets, parent)
            .into_iter()
            .filter(|(n, _)| vars.binary_search_by(|(m, _)| (**m).cmp(n)).is_err())
            .map(|(n, v)| (n.into(), Some(v.into())))
            .collect();
        flat.extend(vars.into_iter().filter(|(_, v)| v.is_some()));
        flat.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        CustomSet {
            parent: NONE,
            depth: 0,
            vars: flat.into(),
        }
    };
    sets.push(set);
    sets.len() as u32 - 1
}

pub(crate) fn has_var(src: &str) -> bool {
    let mut t = Tokenizer::new(src);
    loop {
        let tok = t.next_token();
        match tok.kind {
            Kind::Eof => return false,
            Kind::Function if tok.value_is(src, "var") || tok.value_is(src, "env") => {
                return true;
            }
            _ => {}
        }
    }
}

/// Whether every `var()` and `env()` in `src[span]` is well formed: a
/// declaration with one that is not is invalid when parsed.
pub(crate) fn vars_valid(src: &str, span: Span) -> bool {
    let mut t = Tokenizer::in_span(src, span);
    loop {
        let tok = t.next_token();
        let is_var = tok.value_is(src, "var");
        match tok.kind {
            Kind::Eof => return true,
            Kind::Function if is_var || tok.value_is(src, "env") => {
                match arguments(src, &mut t, is_var) {
                    Some((_, Some(f))) if !vars_valid(src, f) => return false,
                    Some(_) => {}
                    None => return false,
                }
            }
            _ => {}
        }
    }
}

/// The environment variables a page sees in a window with no insets.
fn env(name: &str) -> Option<&'static str> {
    name.starts_with("safe-area-inset-").then_some("0px")
}

/// `src` with every `var()` and `env()` replaced; `None` when one has
/// no value and no fallback, or the result is longer than `MAX_LEN`.
/// With `separate`, each substituted value is
/// kept apart from what is around it by empty comments, so that
/// tokenizing the result gives the tokens substitution gave.
pub(crate) fn substitute(
    src: &str,
    separate: bool,
    get: &mut dyn FnMut(&str) -> Option<String>,
) -> Option<String> {
    let mut out = String::with_capacity(src.len());
    let mut t = Tokenizer::new(src);
    loop {
        let tok = t.next_token();
        let is_var = tok.value_is(src, "var");
        match tok.kind {
            Kind::Eof => return Some(out),
            Kind::Function if is_var || tok.value_is(src, "env") => {
                let (name, fallback) = arguments(src, &mut t, is_var)?;
                let v = if is_var {
                    get(&name)
                } else {
                    env(&name).map(str::to_owned)
                };
                let v = match (v, fallback) {
                    (Some(v), _) => v,
                    (None, Some(f)) => substitute(f.of(src), separate, get)?,
                    (None, None) => return None,
                };
                if separate {
                    out.push_str("/**/");
                }
                out.push_str(&v);
                if separate {
                    out.push_str("/**/");
                }
            }
            _ => out.push_str(tok.span.of(src)),
        }
        if out.len() > MAX_LEN {
            return None;
        }
    }
}

/// After `var(` or `env(`: the name, and the fallback after the comma if
/// there is one; the tokenizer is left after the closing parenthesis.
/// `None` when they are not well formed.
fn arguments(src: &str, t: &mut Tokenizer<'_>, is_var: bool) -> Option<(String, Option<Span>)> {
    let mut name = None;
    let mut fallback: Option<Span> = None;
    let mut depth = 0u32;
    loop {
        let tok = t.next_token();
        match tok.kind {
            // the end closes what is open, at any depth
            Kind::Eof => break,
            Kind::CloseParen if depth == 0 => break,
            Kind::Function | Kind::OpenParen => depth += 1,
            Kind::CloseParen => depth -= 1,
            _ => {}
        }
        if let Some(f) = &mut fallback {
            f.end = tok.span.end;
            continue;
        }
        match tok.kind {
            Kind::Whitespace => {}
            Kind::Ident if name.is_none() => {
                let mut s = String::new();
                tok.write_value(src, &mut s);
                if is_var && !s.starts_with("--") {
                    return None;
                }
                name = Some(s);
            }
            Kind::Comma if name.is_some() => {
                fallback = Some(Span::new(tok.span.end as usize, tok.span.end as usize))
            }
            _ => return None,
        }
    }
    Some((name?, fallback))
}
