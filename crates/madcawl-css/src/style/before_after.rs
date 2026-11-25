//! The computed values of the `::before` and `::after` pseudo-elements, cascaded from their element's rules
//! with the element's values as the parent's.

use madcawl_dom::NodeId;

use crate::selector::PseudoElement;

use super::buckets::Buckets;
use super::compute::{Run, Src};
use super::computed::ComputedStyle;
use super::generated::Content;
use super::styles::Styles;
use super::values::{Display, Float, Position};

/// Which pseudo-elements are styled, and the index of each in a pair.
pub(crate) const PSEUDOS: [PseudoElement; 2] = [PseudoElement::Before, PseudoElement::After];

impl Run<'_, '_> {
    /// The computed values of `el`'s `::before` and `::after` into `out`, those that generate a box: a
    /// `content` that is not `normal` or `none`, and a `display` that is not `none`.
    pub(crate) fn pseudos(&mut self, el: NodeId, out: &mut Styles) {
        out.drop_pseudos(el);
        if out.get(el).is_none() {
            return;
        }
        let mut content = std::mem::take(&mut self.pseudo_candidates);
        self.prep
            .content_buckets
            .candidates(self.m.dom, el, &mut content);
        let mut more = Vec::new();
        self.prep
            .reval_content_buckets
            .candidates(self.m.dom, el, &mut more);
        content.extend_from_slice(&more);
        if content.is_empty() {
            self.pseudo_candidates = content;
            return;
        }
        let mut all = Vec::new();
        self.prep
            .pseudo_buckets
            .candidates(self.m.dom, el, &mut all);
        let saved = std::mem::take(&mut self.candidates);
        for (k, pe) in PSEUDOS.into_iter().enumerate() {
            // `content` is not inherited: no rule giving it, no box
            if self.match_kind(el, pe, k, &content) {
                self.match_kind(el, pe, k, &all);
                self.pseudo_box(el, k, out);
            }
        }
        self.candidates = saved;
        self.pseudo_candidates = content;
    }

    /// Whether a rule of `b` matches `el`'s `::before` or `::after`.
    pub(crate) fn pseudo_rule_matches(&mut self, el: NodeId, b: &Buckets) -> bool {
        if b.is_empty() {
            return false;
        }
        let mut pc = std::mem::take(&mut self.pseudo_candidates);
        b.candidates(self.m.dom, el, &mut pc);
        let kinds = &self.prep.pseudo_buckets.kinds;
        let rules = self.m.rules;
        let any = pc.iter().any(|&c| {
            if !self.may_match(c, el) {
                return false;
            }
            self.m.pseudo.set(Some(PSEUDOS[kinds[c as usize] as usize - 1]));
            self.m.entry(&rules.entries[c as usize], el)
        });
        self.m.pseudo.set(None);
        self.pseudo_candidates = pc;
        any
    }

    /// `el`'s `::before` and `::after` when an element with its `PathKey` was styled before: that one's
    /// `memo` decides them. Elements with a key match the same pseudo-element rules but for those that
    /// need more than the element and its ancestors: without a rule declaring `content` matching the
    /// first, none of those matching this one gives no box; when none of those matched the first nor
    /// matches this one, this one has the first's. Else they are made in full.
    pub(crate) fn pseudos_by_key(&mut self, el: NodeId, memo: PseudoMemo, out: &mut Styles) {
        out.drop_pseudos(el);
        if !memo.content {
            if self.pseudo_rule_matches(el, &self.prep.reval_content_buckets) {
                self.pseudos(el, out);
            }
            return;
        }
        match memo.pair {
            Some(pair) if !self.pseudo_rule_matches(el, &self.prep.reval_pseudo_buckets) => {
                if pair != [0; 2] {
                    out.pseudos.insert(el.index(), pair);
                }
            }
            _ => self.pseudos(el, out),
        }
    }

    /// `el`'s `::before` and `::after` in full, and what a later element with its `PathKey` may take.
    pub(crate) fn pseudos_memo(&mut self, el: NodeId, out: &mut Styles) -> PseudoMemo {
        let content = self.pseudo_rule_matches(el, &self.prep.content_buckets);
        self.pseudos(el, out);
        let reval = content && self.pseudo_rule_matches(el, &self.prep.reval_pseudo_buckets);
        PseudoMemo {
            content,
            pair: (!reval).then(|| out.pseudos.get(&el.index()).copied().unwrap_or_default()),
        }
    }

    // the rules of `of` for pseudo-element `pe` (the `k`th) that `el`'s one matches into `matched`;
    // whether there are any
    fn match_kind(&mut self, el: NodeId, pe: PseudoElement, k: usize, of: &[u32]) -> bool {
        let kinds = &self.prep.pseudo_buckets.kinds;
        self.candidates.clear();
        self.candidates
            .extend(of.iter().filter(|&&c| kinds[c as usize] == 1 + k as u8));
        self.m.pseudo.set(Some(pe));
        self.matched.clear();
        self.rule_matches(el);
        self.m.pseudo.set(None);
        !self.matched.is_empty()
    }

    // the values the rules in `matched` give the `k`th pseudo-element of `el`, by the memo when the same
    // rules gave them under the same values before
    fn pseudo_box(&mut self, el: NodeId, k: usize, out: &mut Styles) {
        self.matched
            .sort_unstable_by_key(|a| std::cmp::Reverse(a.0));
        let i = el.index() as usize;
        let key = PseudoKey {
            parent: out.of[i],
            customs: out.customs[i],
            which: k as u8,
            blocks: self
                .matched
                .iter()
                .map(|(_, imp, src)| block_of(src, *imp))
                .collect(),
        };
        let got = match out.pseudo_memo.get(&key) {
            Some(&v) => v,
            None => {
                let v = self.pseudo_values(el, out);
                out.pseudo_memo.insert(key, v);
                v
            }
        };
        if let Some(v) = got {
            out.set_pseudo(el, k, v);
        }
    }

    // the values of the pseudo-element `matched` is for, its parent being the element, interned; none when
    // it generates no box
    fn pseudo_values(&self, el: NodeId, out: &mut Styles) -> Option<u32> {
        let mut s = self.values(el, Some(el), false, out).0;
        if !matches!(s.rare.content, Content::Items(_)) || s.display == Display::None {
            return None;
        }
        adjust(&mut s, out.get(el).expect("styled above"));
        Some(out.intern(s))
    }
}

/// What the first element styled with a `PathKey` leaves for the later ones about its pseudo-elements.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PseudoMemo {
    /// A rule declaring `content` that needs no more than the element and its ancestors matched
    content: bool,
    /// Its `::before` and `::after` values, as `Styles::pseudos` holds them, when it matched no
    /// pseudo-element rule that needs more
    pair: Option<[u32; 2]>,
}

/// What decides a pseudo-element's values: its element's values and custom properties, which one it is,
/// and the declaration blocks that apply to it, in order, each with whether its important ones are meant.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct PseudoKey {
    parent: u32,
    customs: u32,
    which: u8,
    blocks: Box<[(u32, bool)]>,
}

fn block_of(src: &Src, important: bool) -> (u32, bool) {
    match src {
        Src::Block(b) => (*b, important),
        _ => (u32::MAX, important),
    }
}

// a pseudo-element's box is a child of its element's: blockified as floats and positioned boxes are, and
// as the items of a flex or grid container are
fn adjust(s: &mut ComputedStyle, parent: &ComputedStyle) {
    let out_of_flow = matches!(s.position, Position::Absolute | Position::Fixed);
    if out_of_flow {
        s.float = Float::None;
    }
    let items = parent.display.lays_out_items();
    if items {
        s.float = Float::None;
    }
    if out_of_flow || items || s.float != Float::None {
        s.display = s.display.blockified();
    }
}
