//! A tree built only as far as the page outline reads it: every node's
//! role and visibility, whole records for headings and landmarks, the
//! attribute indexes only once one of those looks into them. Names,
//! states and values of the rest are computed on the first request that
//! needs more, in document order as a whole build does, with the
//! visibility read when the tree was built; so the completed tree is the
//! one a whole build would have made then, as long as neither the DOM
//! nor the styles changed in between.

use madcawl_dom::{Dom, NodeId, NodeKind};

use crate::record::{IN_TREE, Record, VISIBLE};
use crate::role::Role;
use crate::tree::{Inputs, SemTree};

/// What `build_all` computes of each record.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fill {
    Whole,
    /// Roles and visibility; whole records for headings and landmarks
    Outline,
    /// The rest of an outline's records, their visibility kept
    Rest,
}

impl SemTree {
    /// The role and heading level of `n`, unless it has no record of its
    /// own to show.
    pub(crate) fn classify(&self, dom: &Dom, inputs: Inputs, n: NodeId) -> Option<(Role, u8)> {
        let cx = self.cx(dom, inputs);
        let (role, level) = cx.role_of(n);
        // the text of a script, a style, a title is not read
        let unread_text = role == Role::Text
            && dom
                .parent(n)
                .is_some_and(|p| dom.kind(p) == NodeKind::Element && cx.role_of(p).0 == Role::None);
        (role != Role::None && !unread_text).then_some((role, level))
    }

    /// Starts observing the DOM and builds what the page outline reads.
    pub fn build_outline(dom: &mut Dom, inputs: Inputs, page_gen: u32) -> SemTree {
        SemTree::build_as(dom, inputs, page_gen, Fill::Outline)
    }

    /// Only what the page outline reads is computed yet.
    pub fn is_partial(&self) -> bool {
        self.partial
    }

    /// `n`'s record as `fill` computes it.
    pub(crate) fn fill(
        &mut self,
        fill: Fill,
        dom: &Dom,
        inputs: Inputs,
        n: NodeId,
        clear: bool,
    ) -> Record {
        match fill {
            Fill::Whole => self.compute(dom, inputs, n, clear),
            Fill::Outline => self.outline_record(dom, inputs, n, clear),
            Fill::Rest => self.rest_of(dom, inputs, n),
        }
    }

    /// The attribute indexes, built the first time a record looks into
    /// them; until then the outline's records are computed without.
    pub(crate) fn build_indexes(&mut self, dom: &Dom) {
        self.ids.build(dom);
        self.label_for.build(dom);
    }

    /// A lookup found the indexes not built since the last call.
    fn missed(&self) -> bool {
        self.ids.take_missed() | self.label_for.take_missed()
    }

    fn outline_record(&mut self, dom: &Dom, inputs: Inputs, n: NodeId, clear: bool) -> Record {
        let mut class = self.classify(dom, inputs, n);
        if self.missed() {
            self.build_indexes(dom);
            class = self.classify(dom, inputs, n);
        }
        let Some((role, level)) = class else {
            return Record {
                flags: IN_TREE,
                ..Record::default()
            };
        };
        if role.landmark() || role == Role::Heading {
            let r = self.compute(dom, inputs, n, clear);
            if !self.missed() {
                return r;
            }
            // computed without what it looked up: again with the indexes
            self.pool.release(r.name);
            self.pool.release(r.value);
            self.build_indexes(dom);
            return self.compute(dom, inputs, n, clear);
        }
        let mut flags = IN_TREE;
        if self.visible(dom, inputs, n, clear) {
            flags |= VISIBLE;
        }
        Record {
            role,
            level,
            flags,
            ..Record::default()
        }
    }

    /// `n`'s whole record, with the visibility it was built with.
    fn rest_of(&mut self, dom: &Dom, inputs: Inputs, n: NodeId) -> Record {
        let shown = self.records[n.index() as usize].flags & VISIBLE;
        let mut r = self.compute(dom, inputs, n, true);
        r.flags |= shown;
        r
    }

    /// Nothing the tree was built from changed since: the DOM recorded
    /// no change (what it recorded is dropped) and the styles are the
    /// same.
    pub(crate) fn unchanged(&mut self, dom: &mut Dom, inputs: Inputs) -> bool {
        let mut muts = std::mem::take(&mut self.muts);
        let kept = dom.take_mutations(&mut muts) && muts.is_empty();
        muts.clear();
        self.muts = muts;
        kept && inputs.versions().0 == self.inputs_versions.0
    }

    /// Made whole when `whole` asks more than the page outline or what
    /// the outline shows changed; false when the DOM or the styles
    /// changed under the tree, which must then be built again.
    pub(crate) fn settle(&mut self, dom: &mut Dom, inputs: Inputs, whole: bool) -> bool {
        if !self.partial {
            return true;
        }
        if !self.unchanged(dom, inputs) {
            return false;
        }
        if whole || inputs.versions() != self.inputs_versions {
            self.build_indexes(dom);
            self.build_all(dom, inputs, Fill::Rest);
            self.partial = false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use madcawl_dom::Dom;
    use madcawl_html::{DocumentParser, Options, Source, Step};

    use crate::record::VISIBLE;
    use crate::role::Role;
    use crate::tree::{Inputs, SemTree};

    fn parse(html: &str) -> Dom {
        let mut p = DocumentParser::new(Dom::new, Options::default(), Source::default());
        p.feed(html.as_bytes());
        p.end();
        assert_eq!(p.run(), Step::Done);
        p.into_sink()
    }

    /// Each node's record as a view reads it, its strings rather than
    /// their numbers in the pool; only the role and visibility of nodes
    /// the outline writes no line for, when `outline` says so.
    fn records(t: &SemTree, dom: &Dom, outline: bool) -> Vec<String> {
        let doc = dom.document();
        let all = std::iter::once(doc).chain(dom.descendants(doc));
        all.map(|n| match t.record(n) {
            Some(r) if outline && !(r.role.landmark() || r.role == Role::Heading) => {
                format!("{n:?} {:?} {}", r.role, r.flags & VISIBLE)
            }
            Some(r) => format!(
                "{n:?} {:?} {} {} {} {:?} {:?}",
                r.role,
                r.level,
                r.flags,
                r.generation,
                t.name(r),
                t.value(r)
            ),
            None => format!("{n:?} -"),
        })
        .collect()
    }

    const PAGES: [&str; 3] = [
        "<h1>Plain</h1><nav aria-label=N><a href=/>x</a></nav><p id=a>t</p><label for=q>Q</label><input id=q>",
        "<h2 aria-labelledby=later>x</h2><p id=later>Later name</p><label for=i>L</label><input id=i>",
        "<section aria-labelledby=s>a</section><main><h3>H <input value=v></h3></main><span id=s>S</span>",
    ];

    #[test]
    fn the_indexes_wait_for_the_first_record_that_looks_into_them() {
        for (i, html) in PAGES.iter().enumerate() {
            let (mut a, mut b) = (parse(html), parse(html));
            let whole = SemTree::build(&mut a, Inputs::default(), 1);
            let mut part = SemTree::build_outline(&mut b, Inputs::default(), 1);
            assert_eq!(
                part.ids.bytes() == 0,
                i == 0,
                "page {i}: built only when looked into"
            );
            let outline = records(&whole, &a, true);
            assert_eq!(records(&part, &b, true), outline, "page {i}");
            assert!(part.settle(&mut b, Inputs::default(), true));
            assert!(!part.is_partial());
            assert_eq!(
                records(&part, &b, false),
                records(&whole, &a, false),
                "page {i}"
            );
            assert!(
                records(&part, &b, false)
                    .iter()
                    .any(|r| r.contains("\"Later name\""))
                    || i != 1
            );
        }
    }
}
