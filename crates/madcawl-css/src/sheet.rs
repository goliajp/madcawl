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

use crate::Span;
use crate::names::{Name, Names};
use crate::parser::{BlockItem, Declaration, ParsedRule, Parser};
use crate::selector::{Context, SelectorList, parse_nested_selector_list, parse_selector_list};
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

/// Where a list of rules is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The top of the stylesheet: `@import` and `@namespace` may come
    /// first, CDO and CDC are dropped
    Top,
    /// Inside a conditional or layer rule outside any style rule
    Group,
}

struct Builder<'a> {
    src: &'a str,
    names: Names,
    default_ns: Option<Name>,
    prefixes: Vec<(Name, Name)>,
    /// Only `@charset`, `@import`, `@layer` statements and `@namespace`
    /// so far
    preamble: bool,
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

    fn style_rule(
        &mut self,
        prelude: Span,
        block: Span,
        may_nest: bool,
        depth: u32,
    ) -> Option<Rule> {
        let selectors = self.selectors(prelude, depth > 0)?;
        if !may_nest || depth >= MAX_NESTING {
            return Some(Rule::Style(StyleRule {
                selectors,
                body: Body::lazy(block),
                rules: Vec::new(),
            }));
        }
        let (decls, rules) = self.style_body(block, depth + 1);
        Some(Rule::Style(StyleRule {
            selectors,
            body: Body::parsed(block, decls),
            rules,
        }))
    }

    /// The declarations and the nested rules of a style rule's block, or
    /// of a conditional rule nested in one.
    fn style_body(&mut self, block: Span, depth: u32) -> (Vec<Declaration>, Vec<Rule>) {
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
            if !after.is_empty() {
                rules.push(Rule::NestedDeclarations(std::mem::take(&mut after).into()));
            }
            let rule = match rule {
                ParsedRule::Qualified { prelude, block } if depth < MAX_NESTING => {
                    self.style_rule(prelude, block, may_nest, depth)
                }
                ParsedRule::Qualified { .. } => None,
                ParsedRule::At {
                    name,
                    prelude,
                    block,
                } => self.at_rule(name, prelude, block, Some(depth)),
            };
            rules.extend(rule);
        }
        if !after.is_empty() {
            rules.push(Rule::NestedDeclarations(after.into()));
        }
        (decls, rules)
    }

    /// The rules of a block of a conditional or layer rule: a rule list,
    /// or, nested in a style rule (`depth`), a style rule's body whose
    /// leading declarations become the first rule.
    fn group(&mut self, block: Span, depth: Option<u32>) -> Vec<Rule> {
        match depth {
            Some(d) if d < MAX_NESTING => {
                let (decls, mut rules) = self.style_body(block, d + 1);
                if !decls.is_empty() {
                    rules.insert(0, Rule::NestedDeclarations(decls.into()));
                }
                rules
            }
            Some(_) => Vec::new(),
            None => self.rules(block, Place::Group),
        }
    }

    fn at_rule(
        &mut self,
        name: Token,
        prelude: Span,
        block: Option<Span>,
        depth: Option<u32>,
    ) -> Option<Rule> {
        let src = self.src;
        let is = |n: &str| name.value_is(src, n);
        let preamble = std::mem::replace(&mut self.preamble, false);
        let rule = match block {
            Some(b) if is("media") => Rule::Media {
                query: prelude,
                rules: self.group(b, depth),
            },
            Some(b) if is("supports") => Rule::Supports {
                condition: prelude,
                rules: self.group(b, depth),
            },
            Some(b) if is("layer") => Rule::Layer {
                names: prelude,
                rules: Some(self.group(b, depth)),
            },
            None if is("layer") => {
                self.preamble = preamble;
                Rule::Layer {
                    names: prelude,
                    rules: None,
                }
            }
            None if is("import") && preamble && depth.is_none() => {
                self.preamble = true;
                Rule::Import { prelude }
            }
            None if is("namespace") && preamble && depth.is_none() => {
                self.preamble = true;
                return self.namespace(prelude);
            }
            None if is("charset") && depth.is_none() => {
                self.preamble = preamble;
                return None;
            }
            Some(b) if is("font-face") && depth.is_none() => Rule::FontFace(Body::lazy(b)),
            Some(b) if is("page") && depth.is_none() => Rule::Page {
                selector: prelude,
                body: Body::lazy(b),
            },
            Some(b) if (is("keyframes") || is("-webkit-keyframes")) && depth.is_none() => {
                Rule::Keyframes {
                    name: prelude,
                    block: b,
                }
            }
            _ => return None,
        };
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

    fn rules(&mut self, span: Span, place: Place) -> Vec<Rule> {
        let mut p = Parser::new(self.src, span);
        let mut out = Vec::new();
        while let Some(item) = p.next_rule(place == Place::Top) {
            let rule = match item {
                BlockItem::Rule(ParsedRule::Qualified { prelude, block }) => {
                    self.preamble = false;
                    let may_nest = p.block_may_nest();
                    self.style_rule(prelude, block, may_nest, 0)
                }
                BlockItem::Rule(ParsedRule::At {
                    name,
                    prelude,
                    block,
                }) => self.at_rule(name, prelude, block, None),
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
    pub fn parse(source: String) -> Stylesheet {
        let source = source.into_boxed_str();
        let mut b = Builder {
            src: &source,
            names: Names::new(),
            default_ns: None,
            prefixes: Vec::new(),
            preamble: true,
        };
        let rules = b.rules(Span::new(0, source.len()), Place::Top);
        let names = b.names;
        Stylesheet {
            source,
            names,
            rules,
        }
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

    /// How many declaration blocks the sheet has, and how many of them
    /// have been parsed.
    pub fn bodies(&self) -> (usize, usize) {
        let mut n = (0, 0);
        count(&self.rules, &mut n);
        n
    }
}

fn count(rules: &[Rule], n: &mut (usize, usize)) {
    fn body(b: &Body, n: &mut (usize, usize)) {
        n.0 += 1;
        n.1 += b.is_parsed() as usize;
    }
    for r in rules {
        match r {
            Rule::Style(s) => {
                body(&s.body, n);
                count(&s.rules, n);
            }
            Rule::FontFace(b) | Rule::Page { body: b, .. } => body(b, n),
            Rule::Media { rules, .. } | Rule::Supports { rules, .. } => count(rules, n),
            Rule::Layer { rules: Some(r), .. } => count(r, n),
            _ => {}
        }
    }
}
