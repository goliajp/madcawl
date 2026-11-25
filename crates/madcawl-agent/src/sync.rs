//! Bringing the records up to date from the DOM's change record: each
//! change marks the records it can reach — the node, what is below it
//! for a structural or attribute change, the ancestors whose name comes
//! from their content, the elements whose `aria-labelledby` names a
//! changed element, a label's control — and only those are computed
//! again. What a view would show differently comes out as deltas.

use madcawl_dom::{Dom, Mutation, NodeId};
use madcawl_str::atom;

use crate::html_table::is_html;
use crate::record::{IN_TREE, NAME_CONTENT, Record, VISIBLE};
use crate::role::Role;
use crate::tree::{Inputs, SemTree};

/// A change a diff reports, with the generation the reference had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delta {
    Added,
    Removed,
    Changed,
}

fn bit(v: &mut Vec<u64>, i: u32) -> bool {
    let (w, b) = ((i / 64) as usize, i % 64);
    if v.len() <= w {
        v.resize(w + 1, 0);
    }
    let was = v[w] & 1 << b != 0;
    v[w] |= 1 << b;
    was
}

fn test(v: &[u64], i: u32) -> bool {
    v.get((i / 64) as usize)
        .is_some_and(|w| w & 1 << (i % 64) != 0)
}

pub(crate) fn live(dom: &Dom, n: NodeId) -> bool {
    dom.node_at(dom.address(n)).is_some()
}

impl SemTree {
    pub(crate) fn mark(&mut self, n: NodeId) {
        if !bit(&mut self.dirty, n.index()) {
            self.dirty_list.push(n.index());
        }
    }

    fn mark_subtree(&mut self, dom: &Dom, n: NodeId) {
        self.mark(n);
        if live(dom, n) {
            let below: Vec<NodeId> = dom.descendants(n).collect();
            for d in below {
                self.mark(d);
            }
        }
    }

    /// What a change at `from` reaches above it.
    pub(crate) fn mark_up(&mut self, dom: &Dom, from: Option<NodeId>) {
        let mut up = from.filter(|&n| live(dom, n));
        while let Some(a) = up {
            let r = self
                .records
                .get(a.index() as usize)
                .copied()
                .unwrap_or_default();
            if r.has(NAME_CONTENT) {
                self.mark(a);
            }
            self.mark_id_deps(dom, a);
            if is_html(dom, a, atom!("label")) {
                self.mark_label_controls(dom, a);
            }
            let captions = [atom!("legend"), atom!("figcaption"), atom!("caption")];
            if captions.iter().any(|&c| is_html(dom, a, c))
                && let Some(p) = dom.parent(a)
            {
                self.mark(p);
            }
            up = dom.parent(a);
        }
    }

    fn mark_label_controls(&mut self, dom: &Dom, label: NodeId) {
        let target = crate::text::attr_string(dom, label, atom!("for"))
            .and_then(|f| self.ids.first(dom, &f));
        match target {
            Some(t) => self.mark(t),
            None => {
                let below: Vec<NodeId> = dom.descendants(label).collect();
                for d in below {
                    self.mark(d);
                }
            }
        }
    }

    /// The elements whose names an ID or a label's `for` value reaches:
    /// those whose `aria-labelledby` names an ID in `ids`, the labels
    /// whose `for` names it, and the first element with each `for` value
    /// in `fors`.
    fn mark_values(&mut self, dom: &Dom, ids: &[Box<str>], fors: &[Box<str>]) {
        for id in ids {
            let deps = self.refs.get(id).cloned().unwrap_or_default();
            for d in deps {
                self.mark(d);
            }
            let labels = self.label_for.all(id).to_vec();
            for l in labels {
                self.mark(l);
            }
            if let Some(first) = self.ids.first(dom, id) {
                self.mark(first);
            }
        }
        for f in fors {
            if let Some(t) = self.ids.first(dom, f) {
                self.mark(t);
            }
        }
    }

    fn apply(&mut self, dom: &Dom, m: Mutation) {
        let (mut ids, mut fors) = (Vec::new(), Vec::new());
        match m {
            Mutation::Inserted(n) if live(dom, n) => {
                self.ids.add_subtree(dom, n, &mut ids);
                self.label_for.add_subtree(dom, n, &mut fors);
                self.mark_subtree(dom, n);
                self.mark_up(dom, dom.parent(n));
            }
            Mutation::Removed { parent, child } => {
                if live(dom, child) {
                    self.ids.remove_subtree(dom, child, &mut ids);
                    self.label_for.remove_subtree(dom, child, &mut fors);
                }
                self.mark_subtree(dom, child);
                self.mark_up(dom, Some(parent));
            }
            Mutation::Attribute(n) if live(dom, n) => {
                ids.extend(self.ids.value_of(n).map(Box::<str>::from));
                fors.extend(self.label_for.value_of(n).map(Box::<str>::from));
                self.ids.update(dom, n);
                self.label_for.update(dom, n);
                ids.extend(self.ids.value_of(n).map(Box::<str>::from));
                fors.extend(self.label_for.value_of(n).map(Box::<str>::from));
                self.mark_subtree(dom, n);
                self.mark_up(dom, Some(n));
            }
            Mutation::Text(n) if live(dom, n) => {
                self.mark(n);
                self.mark_up(dom, dom.parent(n));
            }
            Mutation::Freed(n) => {
                bit(&mut self.freed, n.index());
                ids.extend(self.ids.value_of(n).map(Box::<str>::from));
                fors.extend(self.label_for.value_of(n).map(Box::<str>::from));
                self.ids.forget(n);
                self.label_for.forget(n);
                self.unregister_refs(n);
                self.controls.forget(n);
                self.mark(n);
            }
            _ => {}
        }
        self.mark_values(dom, &ids, &fors);
    }

    /// The elements that point at `n`'s ID (labelledby, label for).
    fn mark_id_deps(&mut self, dom: &Dom, n: NodeId) {
        if let Some(id) = self.ids.value_of(n).map(Box::<str>::from) {
            self.mark_values(dom, &[id], &[]);
        }
    }

    /// Reads the DOM's changes and computes again the records they reach;
    /// what a view would show differently goes into `out` as (slot, the
    /// reference's generation, delta). False when more changed than the
    /// DOM kept: freed slots may have gone unseen, so the caller builds
    /// the tree again under a new page generation (`Agents::sync`).
    #[must_use]
    pub fn sync(
        &mut self,
        dom: &mut Dom,
        inputs: Inputs,
        out: &mut Vec<(u32, u32, Delta)>,
    ) -> bool {
        let mut muts = std::mem::take(&mut self.muts);
        if !dom.take_mutations(&mut muts) {
            self.muts = muts;
            return false;
        }
        for &m in &muts {
            self.apply(dom, m);
        }
        muts.clear();
        self.muts = muts;
        let versions = inputs.versions();
        if versions != self.inputs_versions {
            self.inputs_versions = versions;
            self.refresh_visibility(dom, inputs);
        }
        let list = std::mem::take(&mut self.dirty_list);
        for &i in &list {
            self.recompute(dom, inputs, i, out);
        }
        self.dirty.clear();
        self.freed.clear();
        self.dirty_list = list;
        self.dirty_list.clear();
        true
    }

    /// Reads every record's visibility again, for new styles or geometry:
    /// a node whose visibility changed is computed again, with what its
    /// visibility reaches — names of ancestors made of their content, and
    /// of the elements whose `aria-labelledby` points into it.
    fn refresh_visibility(&mut self, dom: &Dom, inputs: Inputs) {
        let doc = dom.document();
        let mut clear: Vec<bool> = Vec::new();
        for n in std::iter::once(doc).chain(dom.descendants(doc)) {
            let i = n.index() as usize;
            if clear.len() <= i {
                clear.resize(i + 1, false);
            }
            let up = dom.parent(n).is_some_and(|p| clear[p.index() as usize]);
            clear[i] = up || self.opaque_zero(inputs, dom, n);
            let Some(r) = self.records.get(i).copied().filter(|r| r.has(IN_TREE)) else {
                continue;
            };
            if r.role == Role::None || self.visible(dom, inputs, n, clear[i]) == r.has(VISIBLE) {
                continue;
            }
            self.mark(n);
            self.mark_up(dom, dom.parent(n));
            self.mark_id_deps(dom, n);
        }
    }

    fn recompute(&mut self, dom: &Dom, inputs: Inputs, i: u32, out: &mut Vec<(u32, u32, Delta)>) {
        let n = NodeId::from_index(i).expect("a slot index is never 0");
        let old = self.records.get(i as usize).copied().unwrap_or_default();
        let freed = test(&self.freed, i);
        let mut new = if live(dom, n) && dom.is_connected(n) {
            let clear = self.transparent(inputs, dom, n);
            self.compute(dom, inputs, n, clear)
        } else {
            self.unregister_refs(n);
            Record::default()
        };
        self.put(i, new);
        if freed {
            self.records[i as usize].generation = old.generation.wrapping_add(1);
        }
        new = self.records[i as usize];
        let worth = |r: &Record| {
            r.has(IN_TREE) && !r.role.transparent() && (r.role != Role::Text || r.name != 0)
        };
        match (worth(&old), worth(&new)) {
            (true, false) => out.push((i, old.generation, Delta::Removed)),
            (false, true) => out.push((i, new.generation, Delta::Added)),
            (true, true) if old.generation != new.generation => {
                out.push((i, old.generation, Delta::Removed));
                out.push((i, new.generation, Delta::Added));
            }
            (true, true) if !old.same_view(&new) => out.push((i, new.generation, Delta::Changed)),
            _ => {}
        }
    }
}
