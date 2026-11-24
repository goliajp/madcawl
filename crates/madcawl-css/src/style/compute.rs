//! The cascade proper: for each element, the declarations that apply,
//! sorted by origin and importance, element-attached or not, layer,
//! specificity and order; the winning one per property; and from it,
//! the parent's values and the initial ones, the computed values.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;

use madcawl_dom::{Dom, NodeId, NodeKind};
use madcawl_str::atom;

use crate::Span;
use crate::parser::{BlockItem, parse_declaration_list};

use super::Styles;
use super::computed::ComputedStyle;
use super::custom;
use super::device::Device;
use super::hints::hints;
use super::matching::{Matcher, text};
use super::order::{Cascaded, Key};
use super::prepared::Prepared;
use super::props::{self as props, Decl, Prop, Specified, Wide, parse_block};
use super::resolve::{Cx, apply, fix_font, fix_up};
use super::rules::{NONE, Rules};
use super::shorthand;
use super::states::ElementStates;
use super::vals::items;
use super::values::{Display, Float, Position};
use super::var::substitute;

pub(crate) enum Src {
    Block(u32),
    Inline,
    Hint,
}

fn parse_list(text: &str) -> Box<[Decl]> {
    let decls: Vec<_> = parse_declaration_list(text)
        .into_iter()
        .filter_map(|i| match i {
            BlockItem::Declaration(d) => Some(d),
            _ => None,
        })
        .collect();
    parse_block(text, &decls)
}

/// What the cascade of one document keeps while it styles elements.
pub(crate) struct Run<'r, 'a> {
    pub m: Matcher<'r, 'a>,
    pub prep: &'r Prepared,
    pub inline: HashMap<String, Box<[Decl]>>,
    /// The presentational hints of the element being styled, and the
    /// blocks hints parsed to
    pub hint: Option<String>,
    pub hinted: HashMap<String, Box<[Decl]>>,
    pub candidates: Vec<u32>,
    pub matched: Vec<(Key, bool, Src)>,
    pub device: Device,
    /// Look elements up by `PathKey`
    pub path_key: bool,
    /// Compute every `PathKey` hit again in full and compare
    pub verify: bool,
    pub sig: Vec<u64>,
    /// By class list number, the text numbers of the classes in it that
    /// some selector names
    pub named: HashMap<u32, Vec<u64>>,
}

impl<'r, 'a> Run<'r, 'a> {
    pub fn new(
        dom: &'r Dom,
        rules: &'r Rules<'a>,
        prep: &'r Prepared,
        states: &'r ElementStates,
        device: Device,
    ) -> Self {
        Run {
            m: Matcher {
                dom,
                rules,
                quirks: prep.quirks,
                places: Default::default(),
                states,
            },
            prep,
            inline: HashMap::new(),
            hint: None,
            hinted: HashMap::new(),
            candidates: Vec::new(),
            matched: Vec::new(),
            device,
            path_key: true,
            verify: cfg!(debug_assertions),
            sig: Vec::new(),
            named: HashMap::new(),
        }
    }

    /// Style every element of the document, in tree order.
    pub fn all(&mut self, out: &mut Styles) {
        let dom = self.m.dom;
        for el in dom.descendants(dom.document()) {
            if dom.kind(el) == NodeKind::Element {
                self.style_one(el, out);
            }
        }
    }

    /// The rules that may match `el`, for `collect` and its signature.
    pub(crate) fn candidates(&mut self, el: NodeId) {
        self.prep
            .buckets
            .candidates(self.m.dom, el, &mut self.candidates);
    }

    /// The declarations that apply to `el`, from `candidates`, sorted
    /// highest first.
    pub(crate) fn collect(&mut self, el: NodeId) {
        self.matched.clear();
        let rules = self.m.rules;
        for &c in &self.candidates {
            let e = &rules.entries[c as usize];
            if !self.m.entry(e, el) {
                continue;
            }
            let origin = rules.sheets[e.sheet as usize].origin as u32;
            let layer = self.prep.ranks[e.layer as usize];
            for important in [false, true] {
                let k = Key::new(origin, important, false, layer, e.spec, e.order);
                self.matched.push((k, important, Src::Block(e.block)));
            }
        }
        if let Some(style) = self.m.dom.attribute(el, atom!("style")) {
            let style = text(style);
            if !self.inline.contains_key(&*style) {
                let parsed = parse_list(&style);
                self.inline.insert(style.into_owned(), parsed);
            }
            for important in [false, true] {
                let k = Key::new(2, important, true, 0, 0, 0);
                self.matched.push((k, important, Src::Inline));
            }
        }
        self.hint = hints(self.m.dom, el);
        if let Some(h) = &self.hint {
            if !self.hinted.contains_key(h) {
                self.hinted.insert(h.clone(), parse_list(h));
            }
            // author level, unlayered, before every author rule
            let k = Key::new(2, false, false, self.prep.author_rank, 0, 0);
            self.matched.push((k, false, Src::Hint));
        }
        self.matched
            .sort_unstable_by_key(|a| std::cmp::Reverse(a.0));
    }

    /// The value `d` declares for `p` once `var()` in it is substituted;
    /// `None` when that makes it invalid.
    fn substituted(
        &self,
        p: Prop,
        d: &Decl,
        sets: &[custom::CustomSet],
        set: u32,
        budget: &Cell<usize>,
    ) -> Option<Specified> {
        let raw = d.raw.as_deref().expect("the text var() is in");
        let mut get = |n: &str| custom::lookup(sets, set, n);
        let t = substitute(raw, true, &mut get, budget)?;
        let span = Span::new(0, t.len());
        let v = match d.short {
            None => props::parse(p, &t, span)?,
            Some(sh) => {
                let xs = items(&t, span, 64)?;
                let all = shorthand::parse(sh, &t, &xs)?;
                all.into_iter().find(|(w, _)| *w == d.what)?.1
            }
        };
        (v != Specified::Unresolved).then_some(v)
    }

    /// The computed values of `el` from what `collect` found; its custom
    /// property set goes into `out`.
    pub(crate) fn element(
        &self,
        el: NodeId,
        parent: Option<NodeId>,
        out: &mut Styles,
    ) -> ComputedStyle {
        let initial = ComputedStyle::initial();
        let Cascaded { won, customs, .. } = {
            let from = parent.and_then(|p| out.get(p)).unwrap_or(initial);
            self.cascade(el, from)
        };
        let parent_set = parent.map_or(NONE, |p| out.customs[p.index() as usize]);
        let set = out.memo.element_set(&mut out.sets, parent_set, &customs);
        out.customs[el.index() as usize] = set;
        let from = parent.and_then(|p| out.get(p)).unwrap_or(initial);
        let mut s = ComputedStyle::inheriting(from);
        let cx = self.cx(out, self.m.is_root(el));
        for &p in Prop::ALL {
            let v = match won[p as usize] {
                Some(d) if d.value == Specified::Unresolved => {
                    // invalid at computed-value time: as if unset
                    let b = &out.memo.budget;
                    self.substituted(p, d, &out.sets, set, b).map(Cow::Owned)
                }
                d => d.map(|d| Cow::Borrowed(&d.value)),
            };
            match v.as_deref() {
                None | Some(Specified::Wide(Wide::Unset | Wide::Revert | Wide::RevertLayer)) => {
                    // already the parent's or the initial value
                }
                Some(Specified::Wide(Wide::Inherit)) => s.copy_from(from, p),
                Some(Specified::Wide(Wide::Initial)) => s.copy_from(initial, p),
                Some(v) => {
                    apply(&mut s, p, v, from, &cx);
                }
            }
            if p == Prop::FontSize {
                fix_font(&mut s);
            }
        }
        fix_up(&mut s);
        self.adjust(&mut s, el, out);
        s
    }

    fn cx(&self, out: &Styles, root: bool) -> Cx {
        let d = &self.device;
        Cx {
            root_font: if root { 16.0 } else { out.root_font },
            vw: d.width as f32,
            vh: d.height as f32,
            scale: d.scale as f32,
            dark: d.dark,
        }
    }

    /// The changes CSS Display and CSS 2 make once the values are known:
    /// floats, absolutely positioned boxes, the root, and flex and grid
    /// items are block-level, and flex and grid items do not float.
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
        if items {
            s.float = Float::None;
        }
        if root || out_of_flow || s.float != Float::None || items {
            s.display = s.display.blockified();
        }
    }
}
