//! Selectors Level 4: the parsed form of a selector list, its
//! specificity, and its serialization. Matching is the cascade's.
//!
//! A complex selector is one array, its compound selectors from right to
//! left (the order a matcher walks them), each followed by the
//! combinator that joins it to the compound on its left. Within a
//! compound the simple selectors keep the order they were written in.

mod parse;
mod pseudo;
mod serialize;

pub use pseudo::{PseudoClass, PseudoElement};

use crate::Span;
use crate::names::{Name, Names};

/// What a selector may refer to beyond itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    /// The URL of the stylesheet's default namespace
    pub default_ns: Option<Name>,
    /// `(prefix, URL)` of each `@namespace` with a prefix
    pub prefixes: &'a [(Name, Name)],
    /// Whether `&` may appear: in a rule nested in a style rule
    pub nesting: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectorError;

/// Parse `src[span]` as a selector list.
pub fn parse_selector_list(
    src: &str,
    span: Span,
    names: &mut Names,
    cx: &Context<'_>,
) -> Result<SelectorList, SelectorError> {
    parse::run(src, span, names, cx, false)
}

/// Parse `src[span]` as the selector list of a rule nested in a style
/// rule: each selector may start with a combinator, as in `:has()`,
/// and its last component is that combinator (descendant when none was
/// written), which joins it to the parent rule's elements.
pub fn parse_nested_selector_list(
    src: &str,
    span: Span,
    names: &mut Names,
    cx: &Context<'_>,
) -> Result<SelectorList, SelectorError> {
    parse::run(src, span, names, cx, true)
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectorList(pub Box<[Selector]>);

#[derive(Clone, Debug, PartialEq)]
pub struct Selector {
    pub components: Box<[Component]>,
    /// ids << 20 | classes, attributes and pseudo-classes << 10 | types
    /// and pseudo-elements, each part at most 1023
    pub specificity: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

/// The namespace part of a type, universal or attribute selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    /// No prefix written: the default namespace when there is one
    /// (`Some` of its URL), any namespace when not; for an attribute,
    /// no namespace
    Implicit(Option<Name>),
    /// `*|`
    Any,
    /// `|`: no namespace
    None,
    /// `prefix|`, with the URL it was declared for
    Prefix { prefix: Name, url: Name },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrOp {
    Equals,
    Includes,
    DashMatch,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseFlag {
    /// As the document language says: HTML compares some values ASCII
    /// case-insensitively
    Default,
    /// `i`
    Insensitive,
    /// `s`
    Sensitive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrSelector {
    pub ns: Namespace,
    /// The name as written and in ASCII lowercase
    pub name: Name,
    pub lower: Name,
    /// The operator and the value; `None` for `[name]`
    pub op: Option<(AttrOp, Name, CaseFlag)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NthKind {
    Child,
    LastChild,
    OfType,
    LastOfType,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Nth {
    pub kind: NthKind,
    pub a: i32,
    pub b: i32,
    /// `of S`, for the two child kinds
    pub of: Option<SelectorList>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Ltr,
    Rtl,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Component {
    Combinator(Combinator),
    Universal(Namespace),
    /// A type selector: the name as written and in ASCII lowercase
    Type {
        ns: Namespace,
        name: Name,
        lower: Name,
    },
    Id(Name),
    Class(Name),
    Attribute(Box<AttrSelector>),
    PseudoClass(PseudoClass),
    Nth(Box<Nth>),
    Not(SelectorList),
    Is(SelectorList),
    Where(SelectorList),
    /// Relative selectors: each one's last component is the combinator
    /// that joins it to the element `:has()` is on
    Has(SelectorList),
    Lang(Box<[Name]>),
    Dir(Dir),
    PseudoElement(PseudoElement),
    /// `&`: the parent rule's selector, which the cascade substitutes;
    /// it counts nothing toward specificity here
    Nesting,
}

impl SelectorList {
    pub fn max_specificity(&self) -> u32 {
        self.0.iter().map(|s| s.specificity).max().unwrap_or(0)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Selector> {
        self.0.iter()
    }
}

/// The specificity of `(a, b, c)`, each part capped at 1023.
pub(crate) fn pack(a: u32, b: u32, c: u32) -> u32 {
    (a.min(1023) << 20) | (b.min(1023) << 10) | c.min(1023)
}

pub(crate) fn unpack(s: u32) -> (u32, u32, u32) {
    (s >> 20, (s >> 10) & 1023, s & 1023)
}

pub(crate) fn specificity(components: &[Component]) -> u32 {
    let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
    let mut add = |s: u32| {
        let (x, y, z) = unpack(s);
        a = a.saturating_add(x);
        b = b.saturating_add(y);
        c = c.saturating_add(z);
    };
    for comp in components {
        match comp {
            Component::Id(_) => add(pack(1, 0, 0)),
            Component::Class(_)
            | Component::Attribute(_)
            | Component::PseudoClass(_)
            | Component::Lang(_)
            | Component::Dir(_) => add(pack(0, 1, 0)),
            Component::Type { .. } | Component::PseudoElement(_) => add(pack(0, 0, 1)),
            Component::Nth(n) => {
                add(pack(0, 1, 0));
                if let Some(of) = &n.of {
                    add(of.max_specificity());
                }
            }
            Component::Not(l) | Component::Is(l) | Component::Has(l) => add(l.max_specificity()),
            Component::Where(_)
            | Component::Universal(_)
            | Component::Combinator(_)
            | Component::Nesting => {}
        }
    }
    pack(a, b, c)
}
