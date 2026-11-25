//! Building the box tree from the DOM and the computed values, in one walk with an explicit stack.

use madcawl_css::style::{Display, Float, Position, Styles, WhiteSpaceCollapse};
use madcawl_dom::{Dom, NodeId, NodeKind};

use super::kind::kind_of;
use super::{BoxNode, BoxTree, Kind, NONE};

/// Elements nested deeper than this generate no boxes of their own, as if `display: contents`: their text
/// still lays out, and layout, which recurses over the box tree, stays within a bounded depth.
pub const MAX_DEPTH: usize = 256;

pub(super) struct Builder<'a> {
    pub(super) dom: &'a Dom,
    pub(super) styles: &'a Styles,
    pub(super) t: BoxTree,
    /// Where a split inline box's later children go: its next piece
    forward: Vec<u32>,
    /// Whether an anonymous block may take more inline content
    open_anon: Vec<bool>,
    /// Links of `forward` followed
    steps: u64,
}

impl Builder<'_> {
    pub(super) fn push(&mut self, kind: Kind, node: NodeId) -> u32 {
        let i = self.t.boxes.len() as u32;
        self.t.boxes.push(BoxNode {
            kind,
            node,
            parent: NONE,
            first: NONE,
            last: NONE,
            next: NONE,
            pseudo: 0,
        });
        self.t.inline_children.push(false);
        self.forward.push(NONE);
        self.open_anon.push(false);
        i
    }

    // inline-level for the box tree's shape: floats and out-of-flow boxes do not split inline boxes or
    // wrap their siblings in anonymous blocks
    fn inline_level(&self, c: u32) -> bool {
        let b = self.t.boxes[c as usize];
        b.kind.is_inline_level()
            || b.kind == Kind::Block
                && self.style_of(c).is_some_and(|s| {
                    s.float != Float::None
                        || matches!(s.position, Position::Absolute | Position::Fixed)
                })
    }

    fn link(&mut self, p: u32, c: u32) {
        let last = self.t.boxes[p as usize].last;
        if last == NONE {
            self.t.boxes[p as usize].first = c;
        } else {
            self.t.boxes[last as usize].next = c;
        }
        self.t.boxes[p as usize].last = c;
        self.t.boxes[c as usize].parent = p;
        if self.t.boxes[p as usize].kind.is_block_container() {
            self.t.inline_children[p as usize] = self.inline_level(c);
        }
    }

    // the latest piece of a split inline box; the first piece is then pointed at it, so the children of a
    // box split by many blocks each find it in a step or two
    fn resolve(&mut self, p: u32) -> u32 {
        let mut at = p;
        while self.forward[at as usize] != NONE {
            at = self.forward[at as usize];
            self.steps += 1;
        }
        if at != p {
            self.forward[p as usize] = at;
        }
        at
    }

    // a text node of only white space that collapses away between blocks
    fn collapsible(&self, b: u32) -> bool {
        let bx = self.t.boxes[b as usize];
        if bx.kind != Kind::Text {
            return false;
        }
        let collapse = self
            .dom
            .parent(bx.node)
            .and_then(|p| self.styles.get(p))
            .is_none_or(|s| s.text.white_space_collapse == WhiteSpaceCollapse::Collapse);
        collapse
            && self.dom.text(bx.node).is_none_or(|t| {
                t.units()
                    .all(|u| matches!(u, 0x20 | 0x09 | 0x0a | 0x0c | 0x0d))
            })
    }

    // moves a block container's inline children into a new anonymous block, or drops them when they are
    // only collapsible white space
    fn wrap_children(&mut self, p: u32) {
        let kids: Vec<u32> = self.t.children(p).collect();
        let b = &mut self.t.boxes[p as usize];
        (b.first, b.last) = (NONE, NONE);
        if kids.iter().all(|&k| self.collapsible(k)) {
            return;
        }
        let anon = self.push(Kind::AnonBlock, self.t.boxes[p as usize].node);
        self.link(p, anon);
        for k in kids {
            self.t.boxes[k as usize].next = NONE;
            self.link(anon, k);
        }
        self.open_anon[anon as usize] = true;
    }

    pub(super) fn append(&mut self, p: u32, c: u32) {
        let p = self.resolve(p);
        let pk = self.t.boxes[p as usize].kind;
        let inline = self.inline_level(c);
        if pk == Kind::Inline && !inline {
            return self.split(p, c);
        }
        if !pk.is_block_container() {
            return self.link(p, c);
        }
        let has_kids = self.t.boxes[p as usize].first != NONE;
        // a flex or grid container's children are all blocks: its items, out-of-flow ones and text in
        // anonymous items
        let items =
            pk != Kind::AnonBlock && self.style_of(p).is_some_and(|s| s.display.lays_out_items());
        if items && self.t.boxes[c as usize].kind == Kind::Block {
            let last = self.t.boxes[p as usize].last;
            if last != NONE {
                self.open_anon[last as usize] = false;
            }
            self.link(p, c);
            self.t.inline_children[p as usize] = false;
            return;
        }
        let inline_mode = !items && (!has_kids || self.t.inline_children[p as usize]);
        if !inline {
            if has_kids && inline_mode {
                self.wrap_children(p);
            }
            let last = self.t.boxes[p as usize].last;
            if last != NONE {
                self.open_anon[last as usize] = false;
            }
            return self.link(p, c);
        }
        if inline_mode {
            return self.link(p, c);
        }
        let last = self.t.boxes[p as usize].last;
        if last != NONE && self.open_anon[last as usize] {
            return self.link(last, c);
        }
        if self.collapsible(c) {
            return;
        }
        let anon = self.push(Kind::AnonBlock, self.t.boxes[p as usize].node);
        self.link(p, anon);
        self.open_anon[anon as usize] = true;
        self.link(anon, c);
    }

    // a block-level box inside inline box `p`: it goes to the block container around the inline boxes, and
    // each of them continues in a new piece after it
    fn split(&mut self, p: u32, c: u32) {
        let mut chain = vec![p];
        let mut up = self.t.boxes[p as usize].parent;
        while self.t.boxes[up as usize].kind == Kind::Inline {
            chain.push(up);
            up = self.t.boxes[up as usize].parent;
        }
        if self.t.boxes[up as usize].kind == Kind::AnonBlock {
            up = self.t.boxes[up as usize].parent;
        }
        self.append(up, c);
        for &i in &chain {
            let n = self.t.boxes[i as usize].node;
            self.t.hoisted.push((c, n));
        }
        let mut target = up;
        for &i in chain.iter().rev() {
            let piece = self.push(Kind::Inline, self.t.boxes[i as usize].node);
            self.append(target, piece);
            self.forward[i as usize] = piece;
            target = piece;
        }
    }

    fn element(&mut self, n: NodeId, p: u32, depth: usize, stack: &mut Vec<Frame>) {
        if !self.styles.is_rendered(self.dom, n) {
            return;
        }
        let Some(s) = self.styles.get(n) else { return };
        let root = p == NONE;
        if !root && (s.display == Display::Contents || depth >= MAX_DEPTH) {
            self.generate(n, p, false);
            stack.push(Frame::new(self.dom, n, p, depth + 1).with_after(n));
            return;
        }
        let kind = if root {
            Kind::Block
        } else {
            kind_of(self.dom, n, s)
        };
        let b = self.push(kind, n);
        self.t.sticky |= s.position == madcawl_css::style::Position::Sticky;
        if !root {
            self.append(p, b);
        }
        if !matches!(kind, Kind::Replaced { .. } | Kind::Break) {
            self.generate(n, b, false);
            stack.push(Frame::new(self.dom, n, b, depth + 1).with_after(n));
        }
    }
}

struct Frame {
    next: Option<NodeId>,
    parent: u32,
    depth: usize,
    /// The element whose `::after` comes once its children are done
    after: Option<NodeId>,
}

impl Frame {
    fn new(dom: &Dom, n: NodeId, parent: u32, depth: usize) -> Frame {
        Frame {
            next: dom.first_child(n),
            parent,
            depth,
            after: None,
        }
    }

    fn with_after(self, n: NodeId) -> Frame {
        Frame {
            after: Some(n),
            ..self
        }
    }
}

/// The boxes `dom`'s document generates under `styles`.
pub fn build(dom: &Dom, styles: &Styles) -> BoxTree {
    let mut t = run(dom, styles).t;
    crate::table::fix_tree(&mut t, styles);
    t
}

fn run<'a>(dom: &'a Dom, styles: &'a Styles) -> Builder<'a> {
    let mut b = Builder {
        dom,
        styles,
        t: BoxTree::default(),
        forward: Vec::new(),
        open_anon: Vec::new(),
        steps: 0,
    };
    let Some(root) = dom
        .children(dom.document())
        .find(|&c| dom.kind(c) == NodeKind::Element)
    else {
        return b;
    };
    let mut stack = Vec::new();
    b.element(root, NONE, 0, &mut stack);
    while let Some(f) = stack.last_mut() {
        let Some(n) = f.next else {
            if let Some(el) = f.after {
                let p = f.parent;
                b.generate(el, p, true);
            }
            stack.pop();
            continue;
        };
        f.next = dom.next_sibling(n);
        let (p, depth) = (f.parent, f.depth);
        match dom.kind(n) {
            NodeKind::Element => b.element(n, p, depth, &mut stack),
            NodeKind::Text if styles.is_rendered(dom, n) => {
                let t = b.push(Kind::Text, n);
                b.append(p, t);
            }
            _ => {}
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use madcawl_css::style::{Device, style_document};
    use madcawl_html::{DocumentParser, Options, Source, Step};

    use super::*;

    fn steps(n: usize) -> u64 {
        let html = format!(
            "<!doctype html><body><span>{}</span>",
            "<div>a</div>b".repeat(n)
        );
        let mut p = DocumentParser::new(Dom::new, Options::default(), Source::default());
        p.feed(html.as_bytes());
        p.end();
        assert_eq!(p.run(), Step::Done);
        let dom = p.into_sink();
        let styles = style_document(&dom, Device::default(), &mut |_| None);
        run(&dom, &styles).steps
    }

    #[test]
    fn an_inline_box_split_by_many_blocks_finds_its_latest_piece_directly() {
        let (a, b) = (steps(500), steps(2000));
        assert!(b <= 6 * a.max(1), "{a} then {b}");
    }
}
