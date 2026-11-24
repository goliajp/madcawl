//! The cascade proper: for each element, the declarations that apply,
//! sorted by origin and importance, element-attached or not, layer,
//! specificity and order; the winning one per property; and from it,
//! the parent's values and the initial ones, the computed values.

use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::atom;

use crate::parser::{BlockItem, parse_declaration_list};

use super::buckets::Buckets;
use super::matching::{Matcher, text};
use super::props::{Decl, PROPS, Prop, Specified, Wide, parse_block};
use super::rules::Rules;
use super::values::{ComputedStyle, ContentVisibility, Display, Float, Position};
use super::{CONTENTS_SKIPPED, NOT_RENDERED, Styles};

/// A declared value's place in the cascade, compared as one number:
/// origin and importance, element-attached, layer, specificity, order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key(u128);

impl Key {
    fn new(origin: u32, important: bool, attached: bool, layer: u32, spec: u32, order: u32) -> Key {
        let level = if important { 6 - origin } else { origin + 1 };
        let layer = if important { !layer } else { layer };
        Key(u128::from(level) << 100
            | u128::from(attached) << 96
            | u128::from(layer) << 64
            | u128::from(spec) << 32
            | u128::from(order))
    }

    fn level(self) -> u32 {
        (self.0 >> 100) as u32
    }

    fn origin(self) -> u32 {
        let l = self.level();
        if l <= 3 { l - 1 } else { 6 - l }
    }

    /// Element-attached and layer, as one number
    fn layer(self) -> u64 {
        (self.0 >> 64) as u64 & ((1 << 33) - 1)
    }
}

/// What a `revert` or `revert-layer` already seen leaves a property to.
#[derive(Clone, Copy)]
enum Floor {
    /// Only origins below this one
    Origin(u32),
    /// Only lower layers at this level, or lower levels
    Layer { level: u32, layer: u64 },
}

impl Floor {
    fn allows(self, k: Key) -> bool {
        match self {
            Floor::Origin(o) => k.origin() < o,
            Floor::Layer { level, layer } => k.level() != level || k.layer() < layer,
        }
    }
}

enum Src {
    Block(u32),
    Inline,
}

pub(crate) struct Run<'r, 'a> {
    m: Matcher<'r, 'a>,
    buckets: Buckets,
    ranks: Vec<u32>,
    inline: HashMap<String, Box<[Decl]>>,
    candidates: Vec<u32>,
    matched: Vec<(Key, bool, Src)>,
}

impl<'r, 'a> Run<'r, 'a> {
    pub fn new(dom: &'r Dom, rules: &'r Rules<'a>) -> Self {
        let quirks = dom.quirks_mode() == madcawl_html::QuirksMode::Quirks;
        Run {
            m: Matcher { dom, rules, quirks },
            buckets: Buckets::build(rules, quirks),
            ranks: rules.layer_ranks(),
            inline: HashMap::new(),
            candidates: Vec::new(),
            matched: Vec::new(),
        }
    }

    pub fn all(&mut self, out: &mut Styles) {
        let dom = self.m.dom;
        let mut interned: HashMap<ComputedStyle, u32> = HashMap::new();
        for el in dom.descendants(dom.document()) {
            if dom.kind(el) != NodeKind::Element {
                continue;
            }
            let parent = self.m.parent_element(el);
            let s = self.element(el, parent, out);
            let k = *interned.entry(s).or_insert_with(|| {
                out.table.push(s);
                out.table.len() as u32 - 1
            });
            let i = el.index() as usize;
            if out.of.len() <= i {
                out.of.resize(i + 1, 0);
                out.flags.resize(i + 1, 0);
            }
            out.of[i] = k + 1;
            let above = parent.map_or(0, |p| {
                let f = out.flags[p.index() as usize];
                let hides = out
                    .get(p)
                    .is_some_and(|ps| ps.content_visibility == ContentVisibility::Hidden);
                f | if hides { CONTENTS_SKIPPED } else { 0 }
            });
            let none = if s.display == Display::None {
                NOT_RENDERED
            } else {
                0
            };
            out.flags[i] = above | none;
        }
    }

    fn collect(&mut self, el: NodeId) {
        self.matched.clear();
        self.buckets
            .candidates(self.m.dom, el, &mut self.candidates);
        let rules = self.m.rules;
        for &c in &self.candidates {
            let e = &rules.entries[c as usize];
            if !self.m.entry(e, el) {
                continue;
            }
            let origin = rules.sheets[e.sheet as usize].origin as u32;
            let layer = self.ranks[e.layer as usize];
            for important in [false, true] {
                let k = Key::new(origin, important, false, layer, e.spec, e.order);
                self.matched.push((k, important, Src::Block(e.block)));
            }
        }
        if let Some(style) = self.m.dom.attribute(el, atom!("style")) {
            let style = text(style).into_owned();
            self.inline.entry(style).or_insert_with_key(|s| {
                let decls: Vec<_> = parse_declaration_list(s)
                    .into_iter()
                    .filter_map(|i| match i {
                        BlockItem::Declaration(d) => Some(d),
                        _ => None,
                    })
                    .collect();
                parse_block(s, &decls)
            });
            for important in [false, true] {
                let k = Key::new(2, important, true, 0, 0, 0);
                self.matched.push((k, important, Src::Inline));
            }
        }
        self.matched
            .sort_unstable_by_key(|a| std::cmp::Reverse(a.0));
    }

    fn cascade(&mut self, el: NodeId) -> [Option<Specified>; PROPS] {
        self.collect(el);
        let mut won: [Option<Specified>; PROPS] = [None; PROPS];
        let mut floor: [Option<Floor>; PROPS] = [None; PROPS];
        let style = self
            .m
            .dom
            .attribute(el, atom!("style"))
            .map(|s| text(s).into_owned());
        for (key, important, src) in &self.matched {
            let decls = match src {
                Src::Block(b) => self.m.rules.blocks[*b as usize].get(),
                Src::Inline => &self.inline[style.as_deref().expect("a style attribute")],
            };
            for d in decls.iter().rev() {
                let p = d.prop as usize;
                if d.important != *important
                    || won[p].is_some()
                    || floor[p].is_some_and(|f| !f.allows(*key))
                {
                    continue;
                }
                match d.value {
                    Specified::Wide(Wide::Revert) => floor[p] = Some(Floor::Origin(key.origin())),
                    Specified::Wide(Wide::RevertLayer) => {
                        floor[p] = Some(Floor::Layer {
                            level: key.level(),
                            layer: key.layer(),
                        })
                    }
                    v => won[p] = Some(v),
                }
            }
        }
        won
    }

    fn element(&mut self, el: NodeId, parent: Option<NodeId>, out: &Styles) -> ComputedStyle {
        let won = self.cascade(el);
        let from = parent.and_then(|p| out.get(p)).copied();
        let inherited = from.unwrap_or(ComputedStyle::INITIAL);
        let mut s = ComputedStyle::INITIAL;
        for &p in Prop::ALL {
            let v = won[p as usize];
            let inherit = match v {
                None | Some(Specified::Unresolved | Specified::Wide(Wide::Unset)) => p.inherited(),
                // a revert that found nothing below it
                Some(Specified::Wide(Wide::Revert | Wide::RevertLayer)) => p.inherited(),
                Some(Specified::Wide(Wide::Inherit)) => true,
                Some(Specified::Wide(Wide::Initial)) => false,
                Some(v) => {
                    set(&mut s, v);
                    continue;
                }
            };
            if inherit {
                copy(&mut s, &inherited, p);
            }
        }
        self.adjust(&mut s, el, out);
        s
    }

    /// The changes CSS Display and CSS 2 make once the values are known:
    /// floats, absolutely positioned boxes, the root, and flex and grid
    /// items are block-level.
    fn adjust(&self, s: &mut ComputedStyle, el: NodeId, out: &Styles) {
        let out_of_flow = matches!(s.position, Position::Absolute | Position::Fixed);
        if out_of_flow {
            s.float = Float::None;
        }
        let root = self.m.is_root(el);
        if root && s.display == Display::Contents {
            s.display = Display::Block;
        }
        let mut items = false;
        let mut p = self.m.parent_element(el);
        while let Some(x) = p {
            match out.get(x).map(|ps| ps.display) {
                Some(Display::Contents) => p = self.m.parent_element(x),
                Some(d) => {
                    items = d.lays_out_items();
                    break;
                }
                None => break,
            }
        }
        if root || out_of_flow || s.float != Float::None || items {
            s.display = s.display.blockified();
        }
    }
}

fn set(s: &mut ComputedStyle, v: Specified) {
    match v {
        Specified::Display(d) => s.display = d,
        Specified::Visibility(d) => s.visibility = d,
        Specified::ContentVisibility(d) => s.content_visibility = d,
        Specified::Opacity(o) => s.set_opacity(o),
        Specified::Position(d) => s.position = d,
        Specified::Float(d) => s.float = d,
        Specified::Wide(_) | Specified::Unresolved => unreachable!("not a value"),
    }
}

fn copy(s: &mut ComputedStyle, from: &ComputedStyle, p: Prop) {
    match p {
        Prop::Display => s.display = from.display,
        Prop::Visibility => s.visibility = from.visibility,
        Prop::ContentVisibility => s.content_visibility = from.content_visibility,
        Prop::Opacity => s.set_opacity(from.opacity()),
        Prop::Position => s.position = from.position,
        Prop::Float => s.float = from.float,
    }
}
