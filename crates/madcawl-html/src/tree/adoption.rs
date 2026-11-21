//! The adoption agency algorithm, which closes misnested formatting
//! elements.

use super::stack::{Scope, is_special};
use super::tag::{Name, Tag};
use super::{Active, Open, TreeBuilder, TreeSink};

impl<S: TreeSink> TreeBuilder<S> {
    /// Returns false when the token is to be handled as "any other end
    /// tag" instead.
    pub(crate) fn adoption_agency(&mut self, subject: Tag) -> bool {
        let cur = *self.current();
        if cur.html(subject) && self.active_index(cur.node).is_none() {
            self.pop();
            return true;
        }
        for _ in 0..8 {
            let Some(fe_active) = self.active_after_marker(subject) else {
                return false;
            };
            let Active::Element { node: fe, .. } = self.active[fe_active] else {
                unreachable!()
            };
            let Some(fe_open) = self.open_index(fe) else {
                self.remove_active(fe_active);
                return true;
            };
            if !self.in_scope_by(Scope::Default, |e| e.node == fe) {
                return true;
            }
            let Some(fb_open) = (fe_open + 1..self.open.len()).find(|&i| is_special(&self.open[i]))
            else {
                self.pop_until(|e| e.node == fe);
                self.remove_active(fe_active);
                return true;
            };
            self.adopt(fe_active, fe_open, fb_open);
        }
        true
    }

    fn adopt(&mut self, fe_active: usize, fe_open: usize, fb_open: usize) {
        let common = self.open[fe_open - 1];
        let furthest = self.open[fb_open];
        let mut bookmark = fe_active;
        let mut node_i = fb_open;
        let mut last = furthest.node;
        let mut inner = 0;
        loop {
            inner += 1;
            node_i -= 1;
            let node = self.open[node_i];
            if node_i == fe_open {
                break;
            }
            let mut ai = self.active_index(node.node);
            if inner > 3
                && let Some(i) = ai
            {
                self.remove_active(i);
                if i < bookmark {
                    bookmark -= 1;
                }
                ai = None;
            }
            let Some(ai) = ai else {
                self.remove_open(node_i);
                continue;
            };
            let el = self.recreate_active(ai, false, common.node);
            self.open.set_node(node_i, el);
            self.sink.children_finished(node.node);
            if last == furthest.node {
                bookmark = ai + 1;
            }
            self.sink.append(el, last);
            last = el;
        }
        let at = self.place(Some(common));
        self.insert_at(at, last);

        let fe = self.open[fe_open];
        let fe_active = self
            .active_index(fe.node)
            .expect("the formatting element is listed");
        let new = self.recreate_active(fe_active, false, furthest.node);
        self.sink.reparent_children(furthest.node, new);
        self.sink.append(furthest.node, new);
        let entry = self.active.remove(fe_active);
        if fe_active < bookmark {
            bookmark -= 1;
        }
        self.active.insert(bookmark, entry);
        self.remove_open(fe_open);
        let fb = self
            .open_index(furthest.node)
            .expect("the furthest block is open");
        self.open.insert(
            fb + 1,
            Open {
                node: new,
                ns: fe.ns,
                name: Name::known(fe.name.tag),
                html_ip: false,
            },
        );
    }
}
