//! Parsing a selector list from a span of the source: a style rule's
//! prelude, or the argument of `:is()` and its kin, which is parsed
//! again from its own span. Names are decoded and interned in the
//! stylesheet's `Names`.

use super::{
    AttrOp, AttrSelector, CaseFlag, Combinator, Component, Context, Dir, Namespace, Nth, NthKind,
    PseudoClass, PseudoElement, Selector, SelectorError, SelectorList, specificity,
};
use crate::Span;
use crate::anb::parse_anb;
use crate::names::{Name, Names};
use crate::parser::Parser;
use crate::tokenizer::{Kind, Token};

type Result<T> = std::result::Result<T, SelectorError>;

/// `:is(:is(...))` deeper than this is invalid, so nesting costs a
/// bounded native stack.
const MAX_DEPTH: u32 = 32;

#[derive(Clone, Copy, Default)]
struct Flags {
    /// Invalid selectors are dropped, not fatal (`:is()`, `:where()`)
    forgiving: bool,
    /// Each selector may start with a combinator (`:has()`)
    relative: bool,
    /// Inside a functional pseudo-class: no pseudo-elements
    inner: bool,
    /// Inside `:has()`, which may not contain another
    in_has: bool,
}

/// Parse `src[span]`; `relative` for a nested rule's selectors.
pub(super) fn run(
    src: &str,
    span: Span,
    names: &mut Names,
    cx: &Context<'_>,
    relative: bool,
) -> Result<SelectorList> {
    let mut s = State {
        src,
        names,
        cx,
        scratch: String::new(),
    };
    let f = Flags {
        relative,
        ..Flags::default()
    };
    s.list(span, f, 0)
}

struct State<'a, 'n> {
    src: &'a str,
    names: &'n mut Names,
    cx: &'n Context<'n>,
    scratch: String,
}

fn skip_to_comma(p: &mut Parser<'_>) {
    loop {
        let t = p.peek();
        if matches!(t.kind, Kind::Eof | Kind::Comma) {
            return;
        }
        p.next_token();
        p.skip_block(t.kind);
    }
}

fn is_delim(t: &Token, c: u8) -> bool {
    t.kind == Kind::Delim(c)
}

impl State<'_, '_> {
    fn text(&mut self, t: &Token) -> &str {
        self.scratch.clear();
        t.write_value(self.src, &mut self.scratch);
        &self.scratch
    }

    fn intern(&mut self, t: &Token) -> Name {
        self.text(t);
        self.names.intern(&self.scratch)
    }

    fn intern_lower(&mut self, t: &Token) -> Name {
        self.text(t);
        self.scratch.make_ascii_lowercase();
        self.names.intern(&self.scratch)
    }

    fn list(&mut self, span: Span, f: Flags, depth: u32) -> Result<SelectorList> {
        if depth > MAX_DEPTH {
            return Err(SelectorError);
        }
        let mut p = Parser::new(self.src, span);
        let mut out = Vec::new();
        loop {
            match self.complex(&mut p, f, depth) {
                Ok(s) => out.push(s),
                Err(_) if f.forgiving => skip_to_comma(&mut p),
                Err(e) => return Err(e),
            }
            if p.next_token().kind != Kind::Comma {
                break;
            }
        }
        Ok(SelectorList(out.into()))
    }

    fn complex(&mut self, p: &mut Parser<'_>, f: Flags, depth: u32) -> Result<Selector> {
        p.skip_whitespace();
        let mut lead = Combinator::Descendant;
        if f.relative
            && let Some(c) = combinator(&p.peek())
        {
            lead = c;
            p.next_token();
            p.skip_whitespace();
        }
        let mut compounds: Vec<Vec<Component>> = Vec::new();
        let mut combs = Vec::new();
        let mut pseudo_element = false;
        loop {
            compounds.push(self.compound(p, f, depth, &mut pseudo_element)?);
            let mut ws = false;
            while p.peek().kind == Kind::Whitespace {
                p.next_token();
                ws = true;
            }
            let t = p.peek();
            let c = match t.kind {
                Kind::Eof | Kind::Comma => break,
                _ => match combinator(&t) {
                    Some(c) => {
                        p.next_token();
                        p.skip_whitespace();
                        c
                    }
                    None if ws => Combinator::Descendant,
                    None => return Err(SelectorError),
                },
            };
            if pseudo_element {
                return Err(SelectorError);
            }
            combs.push(c);
        }
        let mut components = Vec::new();
        for (i, compound) in compounds.into_iter().enumerate().rev() {
            components.extend(compound);
            if i > 0 {
                components.push(Component::Combinator(combs[i - 1]));
            }
        }
        if f.relative {
            components.push(Component::Combinator(lead));
        }
        Ok(Selector {
            specificity: specificity(&components),
            components: components.into(),
        })
    }

    fn compound(
        &mut self,
        p: &mut Parser<'_>,
        f: Flags,
        depth: u32,
        pseudo_element: &mut bool,
    ) -> Result<Vec<Component>> {
        let mut out = Vec::new();
        if let Some(c) = self.type_selector(p)? {
            out.push(c);
        }
        loop {
            let t = p.peek();
            let after = *pseudo_element;
            match t.kind {
                Kind::Hash { id: true } if !after => {
                    p.next_token();
                    out.push(Component::Id(self.intern(&t)));
                }
                Kind::Delim(b'.') if !after => {
                    p.next_token();
                    let n = p.next_token();
                    if n.kind != Kind::Ident {
                        return Err(SelectorError);
                    }
                    out.push(Component::Class(self.intern(&n)));
                }
                Kind::OpenSquare if !after => {
                    p.next_token();
                    let end = p.skip_block(Kind::OpenSquare);
                    let span = Span::new(t.span.end as usize, end);
                    out.push(Component::Attribute(Box::new(self.attribute(span)?)));
                }
                Kind::Colon => {
                    p.next_token();
                    out.push(self.pseudo(p, f, depth, pseudo_element)?);
                }
                Kind::Delim(b'&') if self.cx.nesting && !after => {
                    p.next_token();
                    out.push(Component::Nesting);
                }
                _ => break,
            }
        }
        if out.is_empty() {
            return Err(SelectorError);
        }
        Ok(out)
    }

    /// The namespace a prefix token names: `*` or a declared prefix.
    fn prefix(&mut self, t: &Token) -> Result<Namespace> {
        if is_delim(t, b'*') {
            return Ok(Namespace::Any);
        }
        let prefix = self.intern(t);
        let found = self.cx.prefixes.iter().find(|(p, _)| *p == prefix);
        found
            .map(|&(prefix, url)| Namespace::Prefix { prefix, url })
            .ok_or(SelectorError)
    }

    /// An optional `ns|` and the name or `*` after it, when the next
    /// tokens are one; the tokenizer is left after it.
    fn qualified_name(
        &mut self,
        p: &mut Parser<'_>,
        implicit: Namespace,
    ) -> Result<Option<(Namespace, Token)>> {
        let t = p.peek();
        let name_like = |t: &Token| t.kind == Kind::Ident || is_delim(t, b'*');
        if is_delim(&t, b'|') {
            let mark = p.mark();
            p.next_token();
            let n = p.peek();
            if name_like(&n) {
                p.next_token();
                return Ok(Some((Namespace::None, n)));
            }
            p.restore(mark);
            return Ok(None);
        }
        if !name_like(&t) {
            return Ok(None);
        }
        p.next_token();
        if is_delim(&p.peek(), b'|') {
            let mark = p.mark();
            p.next_token();
            let n = p.peek();
            if name_like(&n) {
                p.next_token();
                return Ok(Some((self.prefix(&t)?, n)));
            }
            p.restore(mark);
        }
        Ok(Some((implicit, t)))
    }

    fn type_selector(&mut self, p: &mut Parser<'_>) -> Result<Option<Component>> {
        let implicit = Namespace::Implicit(self.cx.default_ns);
        let Some((ns, t)) = self.qualified_name(p, implicit)? else {
            return Ok(None);
        };
        if is_delim(&t, b'*') {
            return Ok(Some(Component::Universal(ns)));
        }
        let (name, lower) = (self.intern(&t), self.intern_lower(&t));
        Ok(Some(Component::Type { ns, name, lower }))
    }

    fn attribute(&mut self, span: Span) -> Result<AttrSelector> {
        let mut q = Parser::new(self.src, span);
        q.skip_whitespace();
        let Some((ns, t)) = self.qualified_name(&mut q, Namespace::Implicit(None))? else {
            return Err(SelectorError);
        };
        if t.kind != Kind::Ident {
            return Err(SelectorError);
        }
        let (name, lower) = (self.intern(&t), self.intern_lower(&t));
        q.skip_whitespace();
        let op = match q.next_token().kind {
            Kind::Eof => {
                return Ok(AttrSelector {
                    ns,
                    name,
                    lower,
                    op: None,
                });
            }
            Kind::Delim(b'=') => AttrOp::Equals,
            Kind::IncludeMatch => AttrOp::Includes,
            Kind::DashMatch => AttrOp::DashMatch,
            Kind::PrefixMatch => AttrOp::Prefix,
            Kind::SuffixMatch => AttrOp::Suffix,
            Kind::SubstringMatch => AttrOp::Substring,
            _ => return Err(SelectorError),
        };
        q.skip_whitespace();
        let v = q.next_token();
        if !matches!(v.kind, Kind::Ident | Kind::String) {
            return Err(SelectorError);
        }
        let value = self.intern(&v);
        q.skip_whitespace();
        let flag = q.next_token();
        let case = match flag.kind {
            Kind::Eof => CaseFlag::Default,
            Kind::Ident if flag.value_is(self.src, "i") => CaseFlag::Insensitive,
            Kind::Ident if flag.value_is(self.src, "s") => CaseFlag::Sensitive,
            _ => return Err(SelectorError),
        };
        q.skip_whitespace();
        if case != CaseFlag::Default && q.next_token().kind != Kind::Eof {
            return Err(SelectorError);
        }
        Ok(AttrSelector {
            ns,
            name,
            lower,
            op: Some((op, value, case)),
        })
    }

    /// After a `:`: a pseudo-class, or after `::` a pseudo-element.
    fn pseudo(
        &mut self,
        p: &mut Parser<'_>,
        f: Flags,
        depth: u32,
        pseudo_element: &mut bool,
    ) -> Result<Component> {
        let t = p.next_token();
        let element = |pe: Option<PseudoElement>, seen: &mut bool| match pe {
            Some(e) if !f.inner && !*seen => {
                *seen = true;
                Ok(Component::PseudoElement(e))
            }
            _ => Err(SelectorError),
        };
        match t.kind {
            Kind::Colon => {
                let n = p.next_token();
                if n.kind != Kind::Ident {
                    return Err(SelectorError);
                }
                let pe = PseudoElement::from_name(self.text(&n));
                element(pe, pseudo_element)
            }
            Kind::Ident => {
                let name = self.text(&t);
                if let Some(pc) = PseudoClass::from_name(name) {
                    if *pseudo_element && !pc.user_action() {
                        return Err(SelectorError);
                    }
                    return Ok(Component::PseudoClass(pc));
                }
                let pe = PseudoElement::from_name(name).filter(|e| e.legacy());
                element(pe, pseudo_element)
            }
            Kind::Function if !*pseudo_element => {
                let end = p.skip_block(Kind::Function);
                let arg = Span::new(t.span.end as usize, end);
                self.functional(&t, arg, f, depth)
            }
            _ => Err(SelectorError),
        }
    }

    fn functional(&mut self, t: &Token, arg: Span, f: Flags, depth: u32) -> Result<Component> {
        let inner = Flags {
            inner: true,
            in_has: f.in_has,
            ..Flags::default()
        };
        let forgiving = Flags {
            forgiving: true,
            ..inner
        };
        const NAMES: &[&str] = &[
            "not",
            "is",
            "where",
            "has",
            "nth-child",
            "nth-last-child",
            "nth-of-type",
            "nth-last-of-type",
            "lang",
            "dir",
        ];
        let name = NAMES
            .iter()
            .find(|n| t.value_is(self.src, n))
            .ok_or(SelectorError)?;
        Ok(match *name {
            "not" => Component::Not(self.list(arg, inner, depth + 1)?),
            "is" => Component::Is(self.list(arg, forgiving, depth + 1)?),
            "where" => Component::Where(self.list(arg, forgiving, depth + 1)?),
            "has" if !f.in_has => {
                let rel = Flags {
                    relative: true,
                    in_has: true,
                    ..inner
                };
                Component::Has(self.list(arg, rel, depth + 1)?)
            }
            "nth-child" => self.nth(NthKind::Child, arg, inner, depth)?,
            "nth-last-child" => self.nth(NthKind::LastChild, arg, inner, depth)?,
            "nth-of-type" => self.nth(NthKind::OfType, arg, inner, depth)?,
            "nth-last-of-type" => self.nth(NthKind::LastOfType, arg, inner, depth)?,
            "lang" => self.lang(arg)?,
            "dir" => {
                let mut q = Parser::new(self.src, arg);
                q.skip_whitespace();
                let d = q.next_token();
                q.skip_whitespace();
                let dir = match d.kind {
                    Kind::Ident if d.value_is(self.src, "ltr") => Dir::Ltr,
                    Kind::Ident if d.value_is(self.src, "rtl") => Dir::Rtl,
                    _ => return Err(SelectorError),
                };
                if q.next_token().kind != Kind::Eof {
                    return Err(SelectorError);
                }
                Component::Dir(dir)
            }
            _ => return Err(SelectorError),
        })
    }

    fn nth(&mut self, kind: NthKind, arg: Span, inner: Flags, depth: u32) -> Result<Component> {
        let mut of = None;
        if matches!(kind, NthKind::Child | NthKind::LastChild) {
            let mut q = Parser::new(self.src, arg);
            loop {
                let t = q.next_token();
                if t.kind == Kind::Eof {
                    break;
                }
                if t.kind == Kind::Ident && t.value_is(self.src, "of") {
                    of = Some(t.span);
                    break;
                }
                q.skip_block(t.kind);
            }
        }
        let anb_end = of.map_or(arg.end, |s| s.start);
        let (a, b) = parse_anb(self.src, Span::new(arg.start as usize, anb_end as usize))
            .ok_or(SelectorError)?;
        let of = match of {
            Some(s) => {
                let rest = Span::new(s.end as usize, arg.end as usize);
                Some(self.list(rest, inner, depth + 1)?)
            }
            None => None,
        };
        Ok(Component::Nth(Box::new(Nth { kind, a, b, of })))
    }

    fn lang(&mut self, arg: Span) -> Result<Component> {
        let mut q = Parser::new(self.src, arg);
        let mut out = Vec::new();
        loop {
            q.skip_whitespace();
            let t = q.next_token();
            if !matches!(t.kind, Kind::Ident | Kind::String) {
                return Err(SelectorError);
            }
            out.push(self.intern(&t));
            q.skip_whitespace();
            match q.next_token().kind {
                Kind::Eof => return Ok(Component::Lang(out.into())),
                Kind::Comma => {}
                _ => return Err(SelectorError),
            }
        }
    }
}

fn combinator(t: &Token) -> Option<Combinator> {
    match t.kind {
        Kind::Delim(b'>') => Some(Combinator::Child),
        Kind::Delim(b'+') => Some(Combinator::NextSibling),
        Kind::Delim(b'~') => Some(Combinator::SubsequentSibling),
        _ => None,
    }
}
