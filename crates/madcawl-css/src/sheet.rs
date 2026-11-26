//! The stylesheet model. A stylesheet owns the one copy of its source;
//! every rule refers into it by span. Selectors are parsed when the
//! sheet is; a style rule's declarations are not: the parser only finds
//! the block's closing brace, and the block is parsed into declarations
//! the first time something asks (`Stylesheet::declarations`). A block
//! with a `{` or an at-keyword at its own level may hold nested rules,
//! and those are parsed with the sheet, so the rule list is complete
//! without touching any other block.
//!
//! Declarations stay a name and a span of value tokens: typed values are
//! the cascade's. So do media queries, `@supports` conditions, `@import`
//! preludes and `@keyframes` blocks. Unknown at-rules, and at-rules
//! where they may not be, are dropped, as the standards say.

use std::sync::OnceLock;

mod bodies;

use crate::Span;
use crate::names::{Name, Names};
use crate::parser::{BlockItem, Declaration, ParsedRule, Parser};
use crate::selector::{Context, SelectorError, SelectorList, parse_nested_selector_list, parse_selector_list};
use crate::tokenizer::{Kind, Token, Tokenizer};

/// Style rules nested deeper than this are dropped: each level parses
/// its block once more, so the depth bounds the work per byte.
pub const MAX_NESTING: u32 = 32;

#[derive(Debug)]
pub struct Body {
    pub span: Span,
    decls: OnceLock<Box<[Declaration]>>,
}

impl Body {
    fn lazy(span: Span) -> Body {
        Body {
            span,
            decls: OnceLock::new(),
        }
    }

    fn parsed(span: Span, decls: Vec<Declaration>) -> Body {
        let b = Body::lazy(span);
        let _ = b.decls.set(decls.into());
        b
    }

    pub fn is_parsed(&self) -> bool {
        self.decls.get().is_some()
    }
}

#[derive(Debug)]
pub struct StyleRule {
    pub selectors: SelectorList,
    pub body: Body,
    /// Rules nested in the body, in order; declarations that follow a
    /// nested rule come as `Rule::NestedDeclarations`
    pub rules: Vec<Rule>,
}

#[derive(Debug)]
pub enum Rule {
    Style(StyleRule),
    Media {
        query: Span,
        rules: Vec<Rule>,
    },
    Supports {
        condition: Span,
        rules: Vec<Rule>,
    },
    /// `@layer a, b;` has no rules; `@layer a { ... }` has
    Layer {
        names: Span,
        rules: Option<Vec<Rule>>,
    },
    /// The url and the conditions as written
    Import {
        prelude: Span,
    },
    Namespace {
        prefix: Option<Name>,
        url: Name,
    },
    FontFace(Body),
    Page {
        selector: Span,
        body: Body,
    },
    /// The name, and the keyframe rules unparsed
    Keyframes {
        name: Span,
        block: Span,
    },
    /// Declarations in a style rule's body after a nested rule
    NestedDeclarations(Box<[Declaration]>),
}

#[derive(Debug)]
pub struct Stylesheet {
    source: Box<str>,
    names: Names,
    rules: Vec<Rule>,
}

/// Where a rule is: how deep in style and group rules, and whether
/// inside a style rule (where a group rule's block holds declarations
/// too). Every level parses its block once more, so `depth` stays under
/// `MAX_NESTING`.
#[derive(Clone, Copy)]
struct At {
    depth: u32,
    in_style: bool,
}

const TOP: At = At {
    depth: 0,
    in_style: false,
};

/// How far into a stylesheet the rules that must come first go. A rule
/// that is dropped does not move it on.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    /// `@charset`, `@layer` statements and `@import` so far
    Imports,
    /// and `@namespace`
    Namespaces,
    /// Anything else came
    Body,
}

/// A source over 4 GiB, past the positions spans can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceTooLarge;

struct Builder<'a> {
    src: &'a str,
    names: Names,
    default_ns: Option<Name>,
    prefixes: Vec<(Name, Name)>,
    phase: Phase,
}

fn declarations(src: &str, span: Span) -> Vec<Declaration> {
    let mut p = Parser::new(src, span);
    std::iter::from_fn(|| p.next_block_item())
        .filter_map(|i| match i {
            BlockItem::Declaration(d) => Some(d),
            _ => None,
        })
        .collect()
}

impl Builder<'_> {
    fn selectors(&mut self, prelude: Span, nested: bool) -> Option<SelectorList> {
        let cx = Context {
            default_ns: self.default_ns,
            prefixes: &self.prefixes,
            nesting: true,
        };
        let parse = if nested {
            parse_nested_selector_list
        } else {
            parse_selector_list
        };
        parse(self.src, prelude, &mut self.names, &cx).ok()
    }

    fn style_rule(&mut self, prelude: Span, block: Span, may_nest: bool, at: At) -> Option<Rule> {
        let selectors = self.selectors(prelude, at.in_style)?;
        if !may_nest || at.depth + 1 >= MAX_NESTING {
            return Some(Rule::Style(StyleRule {
                selectors,
                body: Body::lazy(block),
                rules: Vec::new(),
            }));
        }
        let inner = At {
            depth: at.depth + 1,
            in_style: true,
        };
        let (decls, rules) = self.style_body(block, inner);
        Some(Rule::Style(StyleRule {
            selectors,
            body: Body::parsed(block, decls),
            rules,
        }))
    }

    /// The declarations and the nested rules of a style rule's block, or
    /// of a group rule nested in one.
    fn style_body(&mut self, block: Span, at: At) -> (Vec<Declaration>, Vec<Rule>) {
        let mut p = Parser::new(self.src, block);
        let (mut decls, mut rules, mut after) = (Vec::new(), Vec::new(), Vec::new());
        while let Some(item) = p.next_block_item() {
            let rule = match item {
                BlockItem::Declaration(d) if rules.is_empty() => {
                    decls.push(d);
                    continue;
                }
                BlockItem::Declaration(d) => {
                    after.push(d);
                    continue;
                }
                BlockItem::Invalid => continue,
                BlockItem::Rule(r) => r,
            };
            let may_nest = p.block_may_nest();
            let rule = match rule {
                ParsedRule::Qualified { prelude, block } => {
                    self.style_rule(prelude, block, may_nest, at)
                }
                ParsedRule::At {
                    name,
                    prelude,
                    block,
                } => self.at_rule(name, prelude, block, at),
            };
            if let Some(rule) = rule {
                if !after.is_empty() {
                    rules.push(Rule::NestedDeclarations(std::mem::take(&mut after).into()));
                }
                rules.push(rule);
            }
        }
        if !after.is_empty() {
            rules.push(Rule::NestedDeclarations(after.into()));
        }
        (decls, rules)
    }

    /// The rules of a group rule's block (`@media`, `@supports`,
    /// `@layer`): a rule list, or in a style rule a style rule's body
    /// whose leading declarations become the first rule. `None` past
    /// the nesting limit, and the rule is dropped.
    fn group(&mut self, block: Span, at: At) -> Option<Vec<Rule>> {
        if at.depth + 1 >= MAX_NESTING {
            return None;
        }
        let inner = At {
            depth: at.depth + 1,
            ..at
        };
        if !at.in_style {
            return Some(self.rules(block, false, inner));
        }
        let (decls, mut rules) = self.style_body(block, inner);
        if !decls.is_empty() {
            rules.insert(0, Rule::NestedDeclarations(decls.into()));
        }
        Some(rules)
    }

    fn at_rule(&mut self, name: Token, prelude: Span, block: Option<Span>, at: At) -> Option<Rule> {
        let src = self.src;
        let is = |n: &str| name.value_is(src, n);
        let top = at.depth == 0;
        let rule = match block {
            Some(b) if is("media") => Rule::Media {
                query: prelude,
                rules: self.group(b, at)?,
            },
            Some(b) if is("supports") => Rule::Supports {
                condition: prelude,
                rules: self.group(b, at)?,
            },
            Some(b) if is("layer") => Rule::Layer {
                names: prelude,
                rules: Some(self.group(b, at)?),
            },
            None if is("layer") => {
                return Some(Rule::Layer {
                    names: prelude,
                    rules: None,
                });
            }
            None if is("import") => {
                return (top && self.phase == Phase::Imports).then_some(Rule::Import { prelude });
            }
            None if is("namespace") => {
                if !top || self.phase > Phase::Namespaces {
                    return None;
                }
                let rule = self.namespace(prelude)?;
                self.phase = Phase::Namespaces;
                return Some(rule);
            }
            Some(b) if is("font-face") && !at.in_style => Rule::FontFace(Body::lazy(b)),
            Some(b) if is("page") && !at.in_style => Rule::Page {
                selector: prelude,
                body: Body::lazy(b),
            },
            Some(b) if (is("keyframes") || is("-webkit-keyframes")) && !at.in_style => {
                Rule::Keyframes {
                    name: prelude,
                    block: b,
                }
            }
            _ => return None,
        };
        if top {
            self.phase = Phase::Body;
        }
        Some(rule)
    }

    /// `@namespace [prefix] <string> | <url>`.
    fn namespace(&mut self, prelude: Span) -> Option<Rule> {
        let mut tok = Tokenizer::in_span(self.src, prelude);
        let mut next = || loop {
            let t = tok.next_token();
            if t.kind != Kind::Whitespace {
                return t;
            }
        };
        let mut t = next();
        let mut prefix = None;
        let mut scratch = String::new();
        if t.kind == Kind::Ident {
            t.write_value(self.src, &mut scratch);
            prefix = Some(self.names.intern(&scratch));
            t = next();
        }
        scratch.clear();
        match t.kind {
            Kind::String | Kind::Url => t.write_value(self.src, &mut scratch),
            Kind::Function if t.value_is(self.src, "url") => {
                let s = next();
                if s.kind != Kind::String || next().kind != Kind::CloseParen {
                    return None;
                }
                s.write_value(self.src, &mut scratch);
            }
            _ => return None,
        }
        if next().kind != Kind::Eof {
            return None;
        }
        let url = self.names.intern(&scratch);
        match prefix {
            Some(p) => self.prefixes.push((p, url)),
            None => self.default_ns = Some(url),
        }
        Some(Rule::Namespace { prefix, url })
    }

    /// A list of rules: the stylesheet's (`top_level`, where CDO and
    /// CDC are dropped) or a group rule's outside any style rule.
    fn rules(&mut self, span: Span, top_level: bool, at: At) -> Vec<Rule> {
        let mut p = Parser::new(self.src, span);
        let mut out = Vec::new();
        while let Some(item) = p.next_rule(top_level) {
            let rule = match item {
                BlockItem::Rule(ParsedRule::Qualified { prelude, block }) => {
                    let may_nest = p.block_may_nest();
                    let rule = self.style_rule(prelude, block, may_nest, at);
                    if rule.is_some() && at.depth == 0 {
                        self.phase = Phase::Body;
                    }
                    rule
                }
                BlockItem::Rule(ParsedRule::At {
                    name,
                    prelude,
                    block,
                }) => self.at_rule(name, prelude, block, at),
                _ => None,
            };
            out.extend(rule);
        }
        out
    }
}

impl Stylesheet {
    /// Parse a stylesheet's text. Touches no state outside the sheet, so
    /// any thread may run it.
    pub fn parse(source: String) -> Result<Stylesheet, SourceTooLarge> {
        if source.len() > u32::MAX as usize {
            return Err(SourceTooLarge);
        }
        let source = source.into_boxed_str();
        let mut b = Builder {
            src: &source,
            names: Names::new(),
            default_ns: None,
            prefixes: Vec::new(),
            phase: Phase::Imports,
        };
        let rules = b.rules(Span::new(0, source.len()), true, TOP);
        let names = b.names;
        Ok(Stylesheet {
            source,
            names,
            rules,
        })
    }

    /// A sheet of one style rule, with no declarations, whose selectors
    /// are the whole of `source`: a selector list on its own, as the
    /// selector queries take one, braces and semicolons in its strings
    /// included.
    pub(crate) fn of_selectors(source: String) -> Result<Stylesheet, SelectorError> {
        let mut names = Names::new();
        let end = source.len();
        let selectors = parse_selector_list(&source, Span::new(0, end), &mut names, &Context::default())?;
        let body = Body::parsed(Span::new(end, end), Vec::new());
        let rules = vec![Rule::Style(StyleRule { selectors, body, rules: Vec::new() })];
        Ok(Stylesheet { source: source.into(), names, rules })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn names(&self) -> &Names {
        &self.names
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// A body's declarations, parsed the first time they are asked for.
    pub fn declarations<'a>(&'a self, body: &'a Body) -> &'a [Declaration] {
        body.decls
            .get_or_init(|| declarations(&self.source, body.span).into())
    }
}
