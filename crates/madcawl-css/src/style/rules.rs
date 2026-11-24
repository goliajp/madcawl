//! A cascade's rules: every style rule whose conditions hold, flattened
//! to one entry per selector, with the cascade layer it is in, its
//! specificity with nesting resolved, and its place in the order of
//! appearance.

use std::cell::OnceCell;

use madcawl_html::Namespace;
use madcawl_str::{Atom, HeldAtoms};

use crate::parser::Declaration;
use crate::selector::{Component, SelectorList};
use crate::sheet::{Rule, Stylesheet};
use crate::tokenizer::Kind;
use crate::{Span, selector};

use super::media::{Device, matches_media};
use super::props::{Decl, parse_block};
use super::supports::supports;

/// Where a stylesheet comes from, lowest precedence first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    UserAgent,
    User,
    Author,
}

pub(crate) const NONE: u32 = u32::MAX;

pub(crate) struct SheetData<'a> {
    pub sheet: &'a Stylesheet,
    pub origin: Origin,
    /// The atom of each of the sheet's names
    pub atoms: Vec<Atom>,
    /// The element namespace each name is the URL of, if one
    pub ns: Vec<Option<Namespace>>,
}

/// A selector as it matches: its components, and whether its last one
/// is the combinator that joins it to the parent rule's elements.
#[derive(Clone, Copy)]
pub(crate) struct Complex<'a> {
    pub comps: &'a [Component],
    pub relative: bool,
}

/// A style rule other rules are nested in.
pub(crate) struct Parent<'a> {
    pub sels: Vec<Complex<'a>>,
    pub parent: u32,
    /// The largest specificity of its selectors, nesting resolved
    pub spec: u32,
}

#[derive(Clone, Copy)]
pub(crate) enum Sel<'a> {
    Complex(Complex<'a>),
    /// Declarations nested after a rule: they match what the parent
    /// rule's selectors match
    Parent,
}

pub(crate) struct Entry<'a> {
    pub sel: Sel<'a>,
    pub sheet: u32,
    pub parent: u32,
    pub block: u32,
    pub spec: u32,
    pub layer: u32,
    pub order: u32,
}

pub(crate) enum Decls<'a> {
    Body(&'a Stylesheet, &'a crate::sheet::Body),
    List(&'a str, &'a [Declaration]),
}

/// A declaration block, its declarations parsed the first time an
/// element matches it.
pub(crate) struct Block<'a> {
    pub decls: Decls<'a>,
    pub parsed: OnceCell<Box<[Decl]>>,
}

impl Block<'_> {
    pub fn get(&self) -> &[Decl] {
        self.parsed.get_or_init(|| match self.decls {
            Decls::Body(s, b) => parse_block(s.source(), s.declarations(b)),
            Decls::List(src, d) => parse_block(src, d),
        })
    }
}

struct Layer {
    name: String,
    parent: u32,
    children: Vec<u32>,
}

#[derive(Default)]
pub(crate) struct Rules<'a> {
    pub sheets: Vec<SheetData<'a>>,
    pub parents: Vec<Parent<'a>>,
    pub entries: Vec<Entry<'a>>,
    pub blocks: Vec<Block<'a>>,
    layers: Vec<Layer>,
    /// The root layer of each origin
    roots: [u32; 3],
    order: u32,
    anonymous: u32,
}

fn ns_of(url: &str) -> Option<Namespace> {
    match url {
        "http://www.w3.org/1999/xhtml" => Some(Namespace::Html),
        "http://www.w3.org/2000/svg" => Some(Namespace::Svg),
        "http://www.w3.org/1998/Math/MathML" => Some(Namespace::MathMl),
        _ => None,
    }
}

/// `a + b` part by part, as specificity adds.
fn add_spec(a: u32, b: u32, times: u32) -> u32 {
    let part = |s: u32, shift: u32| (s >> shift) & 1023;
    let sum = |shift| (part(a, shift) + part(b, shift) * times).min(1023) << shift;
    sum(20) | sum(10) | sum(0)
}

impl<'a> Rules<'a> {
    pub fn new() -> Self {
        let mut r = Rules::default();
        for k in 0..3 {
            r.roots[k] = r.layers.len() as u32;
            r.layers.push(Layer {
                name: String::new(),
                parent: NONE,
                children: Vec::new(),
            });
        }
        r
    }

    pub fn add(&mut self, sheet: &'a Stylesheet, origin: Origin, held: &HeldAtoms, d: &Device) {
        let names = sheet.names();
        let ns = (0..names.len())
            .map(|i| ns_of(names.get(crate::Name::from_index(i))))
            .collect();
        let k = self.sheets.len() as u32;
        self.sheets.push(SheetData {
            sheet,
            origin,
            atoms: names.atoms(held),
            ns,
        });
        let root = self.roots[origin as usize];
        self.walk(k, sheet.rules(), NONE, root, d);
    }

    fn complex(&self, s: &'a selector::Selector, parent: u32) -> (Complex<'a>, u32) {
        let comps = &s.components[..];
        if parent == NONE {
            return (
                Complex {
                    comps,
                    relative: false,
                },
                s.specificity,
            );
        }
        let amps = comps
            .iter()
            .filter(|c| matches!(c, Component::Nesting))
            .count() as u32;
        let mut c = Complex {
            comps,
            relative: true,
        };
        // a nested selector with `&` in it is not relative to the parent
        // unless it starts with a combinator
        if amps > 0
            && let Some((Component::Combinator(selector::Combinator::Descendant), rest)) =
                comps.split_last()
        {
            c = Complex {
                comps: rest,
                relative: false,
            };
        }
        let times = amps + c.relative as u32;
        (
            c,
            add_spec(s.specificity, self.parents[parent as usize].spec, times),
        )
    }

    fn style(
        &mut self,
        k: u32,
        list: &'a SelectorList,
        block: Decls<'a>,
        parent: u32,
        layer: u32,
    ) -> u32 {
        let b = self.blocks.len() as u32;
        self.blocks.push(Block {
            decls: block,
            parsed: OnceCell::new(),
        });
        let mut sels = Vec::with_capacity(list.0.len());
        let mut max = 0;
        let order = self.order;
        self.order += 1;
        for s in list.iter() {
            let (c, spec) = self.complex(s, parent);
            max = max.max(spec);
            sels.push(c);
            self.entries.push(Entry {
                sel: Sel::Complex(c),
                sheet: k,
                parent,
                block: b,
                spec,
                layer,
                order,
            });
        }
        self.parents.push(Parent {
            sels,
            parent,
            spec: max,
        });
        self.parents.len() as u32 - 1
    }

    fn walk(&mut self, k: u32, rules: &'a [Rule], parent: u32, layer: u32, d: &Device) {
        let sheet = self.sheets[k as usize].sheet;
        let src = sheet.source();
        for r in rules {
            match r {
                Rule::Style(s) => {
                    let p = self.style(k, &s.selectors, Decls::Body(sheet, &s.body), parent, layer);
                    self.walk(k, &s.rules, p, layer, d);
                }
                Rule::Media { query, rules } if matches_media(src, *query, d) => {
                    self.walk(k, rules, parent, layer, d)
                }
                Rule::Supports { condition, rules } if supports(src, *condition) => {
                    self.walk(k, rules, parent, layer, d)
                }
                Rule::Layer { names, rules: None } => {
                    for name in layer_names(src, *names) {
                        self.declare(layer, &name);
                    }
                }
                Rule::Layer {
                    names,
                    rules: Some(rules),
                } => {
                    let mut list = layer_names(src, *names);
                    let inner = match list.len() {
                        0 => {
                            self.anonymous += 1;
                            let n = format!("\u{0}{}", self.anonymous);
                            self.declare(layer, &n)
                        }
                        1 => self.declare(layer, &list.remove(0)),
                        _ => continue,
                    };
                    self.walk(k, rules, parent, inner, d);
                }
                Rule::NestedDeclarations(decls) if parent != NONE => {
                    let b = self.blocks.len() as u32;
                    self.blocks.push(Block {
                        decls: Decls::List(src, decls),
                        parsed: OnceCell::new(),
                    });
                    self.entries.push(Entry {
                        sel: Sel::Parent,
                        sheet: k,
                        parent,
                        block: b,
                        spec: self.parents[parent as usize].spec,
                        layer,
                        order: self.order,
                    });
                    self.order += 1;
                }
                _ => {}
            }
        }
    }

    /// The layer `a.b.c` below `under`, made when it is new.
    fn declare(&mut self, under: u32, dotted: &str) -> u32 {
        let mut at = under;
        for part in dotted.split('.') {
            let found = self.layers[at as usize]
                .children
                .iter()
                .copied()
                .find(|&c| self.layers[c as usize].name == part);
            at = match found {
                Some(c) => c,
                None => {
                    let c = self.layers.len() as u32;
                    self.layers.push(Layer {
                        name: part.to_owned(),
                        parent: at,
                        children: Vec::new(),
                    });
                    self.layers[at as usize].children.push(c);
                    c
                }
            };
        }
        at
    }

    /// Each layer's rank among its origin's: a layer's sublayers come
    /// before the rules directly in it, and the unlayered rules last.
    pub fn layer_ranks(&self) -> Vec<u32> {
        let mut rank = vec![0; self.layers.len()];
        for &root in &self.roots {
            let mut next = 0;
            // post-order with an explicit stack: (layer, children done)
            let mut stack = vec![(root, false)];
            while let Some((l, done)) = stack.pop() {
                if done {
                    rank[l as usize] = next;
                    next += 1;
                    continue;
                }
                stack.push((l, true));
                for &c in self.layers[l as usize].children.iter().rev() {
                    stack.push((c, false));
                }
            }
        }
        debug_assert!(
            self.layers
                .iter()
                .all(|l| l.parent != NONE || l.name.is_empty())
        );
        rank
    }
}

/// The names of `@layer a, b.c`, each dotted name as one string.
fn layer_names(src: &str, span: Span) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for t in super::tokens(src, span) {
        match t.kind {
            Kind::Ident => t.write_value(src, &mut cur),
            Kind::Delim(b'.') => cur.push('.'),
            Kind::Comma => out.push(std::mem::take(&mut cur)),
            _ => return Vec::new(),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
