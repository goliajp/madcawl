//! The semantic tree: one `Record` per node in a side table indexed by
//! the node's slot, the string pool the records point into, and the
//! indexes names follow. Built whole when the first agent attaches, then
//! kept up to date from the DOM's change record (`sync.rs`).

use std::collections::HashMap;

use madcawl_css::style::Styles;
use madcawl_dom::{Dom, Mutation, NodeId, NodeKind};
use madcawl_str::atom;

use crate::controls::Controls;
use crate::cx::Cx;
use crate::geom::Geometry;
use crate::index::AttrIndex;
use crate::name::Source;
use crate::pool::Pool;
use crate::record::*;
use crate::role::Role;
use crate::text::{attr_string, collapsed, id_list, text_content};

/// The most DOM changes kept between two syncs; past it the tree is read
/// again whole.
pub const MUTATION_CAP: usize = 1 << 16;

/// What records are computed from besides the DOM.
#[derive(Clone, Copy, Default)]
pub struct Inputs<'a> {
    pub styles: Option<&'a Styles>,
    pub geometry: Option<&'a Geometry>,
}

impl Inputs<'_> {
    /// The versions of the styles and the geometry: when either changes,
    /// every record's visibility is read again.
    pub fn versions(&self) -> (u64, u64) {
        (
            self.styles.map_or(0, Styles::version),
            self.geometry.map_or(0, Geometry::version),
        )
    }
}

pub struct SemTree {
    pub(crate) records: Vec<Record>,
    pub(crate) dirty: Vec<u64>,
    pub(crate) dirty_list: Vec<u32>,
    /// Slots freed since the last sync, by bit
    pub(crate) freed: Vec<u64>,
    pub(crate) pool: Pool,
    pub(crate) ids: AttrIndex,
    pub(crate) label_for: AttrIndex,
    /// By ID: the elements whose `aria-labelledby` names it
    pub(crate) refs: HashMap<Box<str>, Vec<NodeId>>,
    pub(crate) refs_of: HashMap<NodeId, Vec<Box<str>>>,
    pub(crate) page_gen: u32,
    pub(crate) muts: Vec<Mutation>,
    /// The `Inputs::versions` the visibility was last read under
    pub(crate) inputs_versions: (u64, u64),
    /// Control state an agent's actions set
    pub(crate) controls: Controls,
}

impl SemTree {
    /// Starts observing the DOM and builds every record.
    pub fn build(dom: &mut Dom, inputs: Inputs, page_gen: u32) -> SemTree {
        dom.observe(MUTATION_CAP);
        let mut t = SemTree {
            records: Vec::new(),
            dirty: Vec::new(),
            dirty_list: Vec::new(),
            freed: Vec::new(),
            pool: Pool::new(),
            ids: AttrIndex::new(atom!("id"), None),
            label_for: AttrIndex::new(atom!("for"), Some(atom!("label"))),
            refs: HashMap::new(),
            refs_of: HashMap::new(),
            page_gen,
            muts: Vec::new(),
            inputs_versions: inputs.versions(),
            controls: Controls::default(),
        };
        let doc = dom.document();
        t.ids.add_subtree(dom, doc, &mut Vec::new());
        t.label_for.add_subtree(dom, doc, &mut Vec::new());
        t.build_all(dom, inputs);
        t
    }

    /// Stops observing; everything the tree held goes with it.
    pub fn release(self, dom: &mut Dom) {
        dom.unobserve();
    }

    fn build_all(&mut self, dom: &Dom, inputs: Inputs) {
        let doc = dom.document();
        let mut clear: Vec<bool> = Vec::new();
        let mut order = vec![doc];
        order.extend(dom.descendants(doc));
        for n in order {
            let i = n.index() as usize;
            if clear.len() <= i {
                clear.resize(i + 1, false);
            }
            let up = dom.parent(n).is_some_and(|p| clear[p.index() as usize]);
            clear[i] = up || self.opaque_zero(inputs, dom, n);
            let r = self.compute(dom, inputs, n, clear[i]);
            self.put(n.index(), r);
        }
    }

    pub(crate) fn opaque_zero(&self, inputs: Inputs, dom: &Dom, n: NodeId) -> bool {
        dom.kind(n) == NodeKind::Element
            && inputs
                .styles
                .and_then(|s| s.get(n))
                .is_some_and(|s| s.opacity() == 0.0)
    }

    /// Fully transparent through an ancestor's `opacity: 0`.
    pub(crate) fn transparent(&self, inputs: Inputs, dom: &Dom, n: NodeId) -> bool {
        let mut up = Some(n);
        while let Some(p) = up {
            if self.opaque_zero(inputs, dom, p) {
                return true;
            }
            up = dom.parent(p);
        }
        false
    }

    pub(crate) fn put(&mut self, i: u32, mut r: Record) {
        let i = i as usize;
        if self.records.len() <= i {
            self.records.resize(i + 1, Record::default());
        }
        let old = self.records[i];
        r.generation = old.generation;
        self.pool.release(old.name);
        self.pool.release(old.value);
        self.records[i] = r;
    }

    pub(crate) fn cx<'a>(&'a self, dom: &'a Dom, inputs: Inputs<'a>) -> Cx<'a> {
        Cx {
            dom,
            styles: inputs.styles,
            ids: &self.ids,
            label_for: &self.label_for,
            controls: Some(&self.controls),
        }
    }

    /// The record of `n`, a node in the document; its pool strings are
    /// counted for it.
    pub(crate) fn compute(&mut self, dom: &Dom, inputs: Inputs, n: NodeId, clear: bool) -> Record {
        let computed = {
            let cx = self.cx(dom, inputs);
            let (role, level) = cx.role_of(n);
            // the text of a script, a style, a title is not read
            let unread_text = role == Role::Text
                && dom.parent(n).is_some_and(|p| {
                    dom.kind(p) == NodeKind::Element && cx.role_of(p).0 == Role::None
                });
            if role == Role::None || unread_text {
                None
            } else {
                let (name, src) = cx.name_of(n, role);
                let (flags, value) = cx.states(n, role);
                Some((role, level, name, src, flags, value))
            }
        };
        let Some((role, level, name, src, flags, value)) = computed else {
            self.unregister_refs(n);
            return Record {
                flags: IN_TREE,
                ..Record::default()
            };
        };
        let mut flags = flags | IN_TREE;
        if self.visible(dom, inputs, n, clear) {
            flags |= VISIBLE;
        }
        if role.name_from_content() || src == Source::Content {
            flags |= NAME_CONTENT;
        }
        // what a user reads on a button or link is its text: a name from an
        // attribute that says something else is marked
        if src == Source::Attribute && role.name_from_content() {
            let shown = collapsed(&text_content(dom, n));
            if !shown.is_empty() && shown != name {
                flags |= NAME_ATTR;
            }
        }
        self.register_refs(dom, n);
        Record {
            name: self.pool.intern(&name),
            value: value.map_or(0, |v| self.pool.intern(&v)),
            generation: 0,
            role,
            level,
            flags,
        }
    }

    pub(crate) fn visible(&self, dom: &Dom, inputs: Inputs, n: NodeId, clear: bool) -> bool {
        if clear || inputs.styles.is_some_and(|s| !s.is_visible(dom, n)) {
            return false;
        }
        match inputs.geometry.and_then(|g| g.place(n)) {
            Some(p) => p.shown,
            None => true,
        }
    }

    /// Drops what `n`'s `aria-labelledby` named: it left the tree, was
    /// freed, or is computed again.
    pub(crate) fn unregister_refs(&mut self, n: NodeId) {
        if let Some(old) = self.refs_of.remove(&n) {
            for id in old {
                if let Some(v) = self.refs.get_mut(&id) {
                    v.retain(|&x| x != n);
                    if v.is_empty() {
                        self.refs.remove(&id);
                    }
                }
            }
        }
    }

    fn register_refs(&mut self, dom: &Dom, n: NodeId) {
        self.unregister_refs(n);
        let Some(v) = attr_string(dom, n, atom!("aria-labelledby")) else {
            return;
        };
        let ids: Vec<Box<str>> = id_list(&v).map(Box::from).collect();
        for id in &ids {
            self.refs.entry(id.clone()).or_default().push(n);
        }
        self.refs_of.insert(n, ids);
    }

    pub fn record(&self, n: NodeId) -> Option<&Record> {
        self.records
            .get(n.index() as usize)
            .filter(|r| r.has(IN_TREE))
    }

    pub fn name(&self, r: &Record) -> &str {
        self.pool.get(r.name)
    }

    pub fn value(&self, r: &Record) -> &str {
        self.pool.get(r.value)
    }

    pub fn page_gen(&self) -> u32 {
        self.page_gen
    }

    /// Every byte the tree holds.
    pub fn bytes(&self) -> usize {
        let refs: usize = self.refs_of.values().map(|v| v.len() * 40 + 48).sum();
        self.records.capacity() * size_of::<Record>()
            + (self.dirty.capacity() + self.freed.capacity()) * 8
            + self.dirty_list.capacity() * 4
            + self.pool.bytes()
            + self.ids.bytes()
            + self.label_for.bytes()
            + refs * 2
            + self.muts.capacity() * size_of::<Mutation>()
            + self.controls.bytes()
    }
}
